use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use futures::executor::block_on;
use opamp_client::{OpAMPClient, OpAMPClientHandle};

use crate::agent::error::AgentError;
use crate::{
    command::stream::Event,
    config::agent_configs::AgentID,
    config::{
        agent_configs::SuperAgentConfig, agent_type::OnHost, agent_type_registry::AgentRepository,
    },
    supervisor::{
        error::ProcessError,
        runner::{Running, Stopped, SupervisorRunner},
        supervisor_config::Config,
        Handle, Runner,
    },
};

use super::opamp_builder::OpAMPClientBuilder;

#[derive(Default)]
pub struct SupervisorGroup<C, S>(HashMap<AgentID, (C, Vec<SupervisorRunner<S>>)>);

impl<C> SupervisorGroup<C, Stopped>
where
    C: OpAMPClient,
{
    pub fn new<Repo: AgentRepository, OpAMPBuilder: OpAMPClientBuilder>(
        tx: Sender<Event>,
        cfg: &SuperAgentConfig,
        effective_agent_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        let builder = SupervisorGroupBuilder {
            tx,
            cfg: cfg.clone(),
            effective_agent_repository,
            opamp_builder: opamp_client_builder,
        };
        builder.build()
    }

    pub fn run(self) -> SupervisorGroup<C::Handle, Running> {
        let running = self
            .0
            .into_iter()
            .map(|(t, runners)| {
                let client = block_on(runners.0.start()).unwrap();
                let mut running_runners = Vec::new();

                for runner in runners.1 {
                    running_runners.push(runner.run());
                }
                (t, (client, running_runners))
            })
            .collect();
        SupervisorGroup(running)
    }
}

type WaitResult = Result<(), ProcessError>;

impl<C> SupervisorGroup<C, Running>
where
    C: OpAMPClientHandle,
{
    pub fn wait(self) -> HashMap<AgentID, Vec<WaitResult>> {
        // collect runners wait result
        self.0
            .into_iter()
            .map(|(t, runners)| {
                // stop the OpAMP client
                // TODO: propagate error?
                block_on(runners.0.stop()).unwrap();
                let mut waiting_runners = Vec::new();
                for runner in runners.1 {
                    waiting_runners.push(runner.wait());
                }
                (t, waiting_runners)
            })
            .collect()
    }

    pub fn stop(self) -> HashMap<AgentID, Vec<JoinHandle<()>>> {
        self.0
            .into_iter()
            .map(|(t, runners)| {
                // stop the OpAMP client
                block_on(runners.0.stop()).unwrap();
                let mut stopped_runners = Vec::new();
                for runner in runners.1 {
                    stopped_runners.push(runner.stop());
                }
                (t, stopped_runners)
            })
            .collect()
    }
}

struct SupervisorGroupBuilder<Repo, OpAMPBuilder> {
    tx: Sender<Event>,
    cfg: SuperAgentConfig,
    effective_agent_repository: Repo,
    opamp_builder: OpAMPBuilder,
}

impl<Repo, OpAMPBuilder> SupervisorGroupBuilder<Repo, OpAMPBuilder>
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
{
    fn build(&self) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        let agent_runners = self
            .cfg
            .agents
            .keys()
            .map(|agent_t| {
                let agent = self
                    .effective_agent_repository
                    .get(&agent_t.clone().get())?;

                let on_host = agent
                    .runtime_config
                    .deployment
                    .on_host
                    .clone()
                    .ok_or(AgentError::SupervisorGroupError)?;

                let (id, runner) = build_on_host_runners(&self.tx, agent_t, on_host);
                Ok((id, (self.opamp_builder.build()?, runner)))
            })
            .collect();

        match agent_runners {
            Err(e) => Err(e),
            Ok(agent_runners) => Ok(SupervisorGroup(agent_runners)),
        }
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

// #[cfg(test)]
// pub mod tests {
//     use std::{collections::HashMap, sync::mpsc::Sender};
//
//     use mockall::mock;
//
//     use crate::agent::error::AgentError;
//     use crate::config::agent_type::RuntimeConfig;
//     use crate::{
//         command::stream::Event,
//         config::agent_configs::{AgentID, AgentSupervisorConfig, SuperAgentConfig},
//         config::agent_type::{Agent, Deployment, Executable, OnHost},
//         config::agent_type_registry::{AgentRepository, LocalRepository},
//         supervisor::runner::{
//             sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
//         },
//     };
//
//     // new_sleep_supervisor_group returns a stopped supervisor group with 2 runners with
//     // generic agents one with one exec and the other with 2
//     pub fn new_sleep_supervisor_group(
//         tx: Sender<Event>,
//     ) -> Result<SupervisorGroup<Stopped>, AgentError> {
//         let group: HashMap<AgentID, Vec<SupervisorRunner<Stopped>>> = HashMap::from([
//             (
//                 AgentID("sleep_5".to_string()),
//                 vec![new_sleep_supervisor(tx.clone(), 5)],
//             ),
//             (
//                 AgentID("sleep_10".to_string()),
//                 vec![
//                     new_sleep_supervisor(tx.clone(), 10),
//                     new_sleep_supervisor(tx.clone(), 10),
//                 ],
//             ),
//         ]);
//         Ok(SupervisorGroup(group))
//     }
//
//     #[test]
//     fn new_supervisor_group_from() {
//         let (tx, _) = std::sync::mpsc::channel();
//         let agent_config = SuperAgentConfig {
//             agents: HashMap::from([(
//                 AgentID("agent".to_string()),
//                 AgentSupervisorConfig {
//                     agent_type: "".to_string(),
//                     values_file: "".to_string(),
//                 },
//             )]),
//         };
//
//         let mut builder = SupervisorGroupBuilder {
//             tx,
//             cfg: agent_config.clone(),
//             effective_agent_repository: LocalRepository::default(),
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
// }
