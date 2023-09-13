use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use thiserror::Error;

use crate::config::agent_type_registry::AgentRepositoryError;
use crate::{
    command::stream::Event,
    config::agent_configs::AgentID,
    config::{
        agent_configs::SuperAgentConfig, agent_type::OnHost, agent_type_registry::AgentRepository,
    },
    supervisor::{
        runner::{Running, Stopped, SupervisorRunner},
        supervisor_config::Config,
        Handle, Runner,
    },
};

pub mod supervisor_opamp_group;

#[derive(Error, Debug)]
pub enum SupervisorGroupError {
    #[error("agent repository error: `{0}`")]
    AgentRepositoryError(#[from] AgentRepositoryError),

    #[error("no on_host deployment configuration not found")]
    OnHostNotFound,

    #[error("could create OpAMP builder")]
    OpAMPBuilder,
}

pub trait SupervisorGroup {
    type Started: StartedSupervisorGroup;
    fn run(self) -> Result<Self::Started, SupervisorGroupError>;
}

pub trait StartedSupervisorGroup {
    fn stop(self) -> Result<HashMap<AgentID, Vec<JoinHandle<()>>>, SupervisorGroupError>;
}

pub struct SupervisorGroupWithoutOpAMP<S>(HashMap<AgentID, Vec<SupervisorRunner<S>>>);

impl SupervisorGroupWithoutOpAMP<Stopped> {
    pub fn new<Repo>(
        repo: Repo,
        tx: Sender<Event>,
        cfg: SuperAgentConfig,
    ) -> Result<Self, SupervisorGroupError>
    where
        Repo: AgentRepository,
    {
        let agent_runners = cfg
            .agents
            .keys()
            .map(|agent_t| {
                let agent = repo.get(&agent_t.clone().get())?;

                let on_host = agent
                    .runtime_config
                    .deployment
                    .on_host
                    .clone()
                    .ok_or(SupervisorGroupError::OnHostNotFound)?;

                let (id, runner) = build_on_host_runners(&tx, agent_t, on_host);
                Ok((id.clone(), runner))
            })
            .collect();

        match agent_runners {
            Err(e) => Err(e),
            Ok(agent_runners) => Ok(SupervisorGroupWithoutOpAMP(agent_runners)),
        }
    }
}

impl SupervisorGroup for SupervisorGroupWithoutOpAMP<Stopped> {
    type Started = SupervisorGroupWithoutOpAMP<Running>;
    fn run(self) -> Result<Self::Started, SupervisorGroupError> {
        let running = self
            .0
            .into_iter()
            .map(|(t, runners)| {
                let mut running_runners = Vec::new();

                for runner in runners {
                    running_runners.push(runner.run());
                }
                (t, running_runners)
            })
            .collect();
        Ok(SupervisorGroupWithoutOpAMP(running))
    }
}

impl StartedSupervisorGroup for SupervisorGroupWithoutOpAMP<Running> {
    fn stop(self) -> Result<HashMap<AgentID, Vec<JoinHandle<()>>>, SupervisorGroupError> {
        Ok(self
            .0
            .into_iter()
            .map(|(t, runners)| {
                let mut stopped_runners = Vec::new();
                for runner in runners {
                    stopped_runners.push(runner.stop());
                }
                (t, stopped_runners)
            })
            .collect())
    }
}

fn build_on_host_runners(
    tx: &Sender<Event>,
    agent_t: &AgentID,
    on_host: OnHost,
) -> (AgentID, Vec<SupervisorRunner>) {
    let mut runners = Vec::new();
    for exec in on_host.executables {
        let runner = SupervisorRunner::from(&Config::new(
            exec.path,
            exec.args.into_vector(),
            exec.env.into_map(),
            tx.clone(),
            on_host.restart_policy.clone(),
        ));
        runners.push(runner);
    }
    (agent_t.clone(), runners)
}

#[cfg(test)]
pub mod tests {
    use mockall::mock;

    use super::*;

    use std::collections::HashMap;

    use crate::config::agent_configs::AgentID;

    mock! {
        pub StartedSupervisorGroupMock {}

        impl StartedSupervisorGroup for StartedSupervisorGroupMock {
            fn stop(self) -> Result<HashMap<AgentID, Vec<JoinHandle<()>>>, SupervisorGroupError>;
        }
    }

    mock! {
        pub SupervisorGroupMock {}

        impl SupervisorGroup for SupervisorGroupMock {
         type Started = MockStartedSupervisorGroupMock;

          fn run(self) -> Result<<Self as SupervisorGroup>::Started, SupervisorGroupError>;
        }
    }
    //
    //     // new_sleep_supervisor_group returns a stopped supervisor group with 2 runners with
    //     // generic agents one with one exec and the other with 2
    //     pub fn new_sleep_supervisor_group<B: OpAMPClientBuilder>(
    //         tx: Sender<Event>,
    //         builder: B,
    //     ) -> Result<SupervisorGroupWithoutOpAMP<B::Client, Stopped>, AgentError> {
    //         let group: HashMap<AgentID, (B::Client, Vec<SupervisorRunner<Stopped>>)> = HashMap::from([
    //             (
    //                 AgentID("sleep_5".to_string()),
    //                 (
    //                     builder
    //                         .build(StartSettings {
    //                             instance_id: "testing".to_string(),
    //                             capabilities: Capabilities::default(),
    //                         })
    //                         .unwrap(),
    //                     vec![new_sleep_supervisor(tx.clone(), 5)],
    //                 ),
    //             ),
    //             (
    //                 AgentID("sleep_10".to_string()),
    //                 (
    //                     builder
    //                         .build(StartSettings {
    //                             instance_id: "testing".to_string(),
    //                             capabilities: Capabilities::default(),
    //                         })
    //                         .unwrap(),
    //                     vec![
    //                         new_sleep_supervisor(tx.clone(), 10),
    //                         new_sleep_supervisor(tx.clone(), 10),
    //                     ],
    //                 ),
    //             ),
    //         ]);
    //         Ok(SupervisorGroupWithoutOpAMP(group))
    //     }
    //
    //     #[test]
    //     fn new_supervisor_group_build() {
    //         let (tx, _) = std::sync::mpsc::channel();
    //         let agent_config = SuperAgentConfig {
    //             agents: HashMap::from([(
    //                 AgentID("agent".to_string()),
    //                 AgentSupervisorConfig {
    //                     agent_type: "".to_string(),
    //                     values_file: "".to_string(),
    //                 },
    //             )]),
    //             opamp: crate::config::agent_configs::OpAMPClientConfig::default(),
    //         };
    //
    //         let mut opamp_builder = MockOpAMPClientBuilderMock::new();
    //         opamp_builder
    //             .expect_build()
    //             .once()
    //             .return_once(|_| Ok(MockOpAMPClientMock::new()));
    //
    //         let mut instance_id_getter = MockInstanceIDGetterMock::new();
    //         instance_id_getter
    //             .expect_get()
    //             .times(1)
    //             .returning(|name| name);
    //
    //         let mut builder = SupervisorGroupBuilder {
    //             tx,
    //             cfg: agent_config.clone(),
    //             effective_agent_repository: LocalRepository::default(),
    //             opamp_builder,
    //             instance_id_getter,
    //         };
    //
    //         // Case with no valid key
    //         let supervisor_group = builder.build();
    //         assert_eq!(true, supervisor_group.is_err());
    //
    //         // Case with valid key but not value
    //         _ = builder.effective_agent_repository.store_with_key(
    //             "agent".to_string(),
    //             Agent {
    //                 metadata: Default::default(),
    //                 variables: Default::default(),
    //                 runtime_config: Default::default(),
    //             },
    //         );
    //         let supervisor_group = builder.build();
    //         assert_eq!(true, supervisor_group.is_err());
    //
    //         // Valid case with valid full data
    //         _ = builder.effective_agent_repository.store_with_key(
    //             "agent".to_string(),
    //             Agent {
    //                 metadata: Default::default(),
    //                 variables: Default::default(),
    //                 runtime_config: RuntimeConfig {
    //                     deployment: Deployment {
    //                         on_host: Some(OnHost {
    //                             executables: vec![Executable {
    //                                 path: "a-path".to_string(),
    //                                 args: Default::default(),
    //                                 env: Default::default(),
    //                             }],
    //                             restart_policy: Default::default(),
    //                         }),
    //                     },
    //                 },
    //             },
    //         );
    //
    //         let supervisor_group = builder.build();
    //         assert_eq!(supervisor_group.unwrap().0.iter().count(), 1)
    //     }
}
