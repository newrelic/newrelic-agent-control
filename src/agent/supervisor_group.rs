use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{capabilities, OpAMPClient, OpAMPClientHandle};

use crate::agent::error::AgentError;
use crate::agent::instance_id::InstanceIDGetter;
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

use crate::opamp::client_builder::OpAMPClientBuilder;

#[derive(Default)]
pub struct SupervisorGroup<C, S>(HashMap<AgentID, (Option<C>, Vec<SupervisorRunner<S>>)>);

impl<C> SupervisorGroup<C, Stopped>
where
    C: OpAMPClient,
{
    pub fn new<Repo, OpAMPBuilder, ID>(
        effective_agent_repository: Repo,
        tx: Sender<Event>,
        cfg: SuperAgentConfig,
        opamp_builder: Option<OpAMPBuilder>,
        instance_id_getter: ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError>
    where
        Repo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        let agent_runners = cfg
            .agents
            .keys()
            .map(|agent_t| {
                let agent = effective_agent_repository.get(&agent_t.clone().get())?;

                let on_host = agent
                    .runtime_config
                    .deployment
                    .on_host
                    .clone()
                    .ok_or(AgentError::SupervisorGroupError)?;

                let (id, runner) = build_on_host_runners(&tx, agent_t, on_host);
                let opamp_client = match &opamp_builder {
                    Some(builder) => Some(builder.build(StartSettings {
                        instance_id: instance_id_getter.get(id.clone().get()),
                        capabilities: capabilities!(AgentCapabilities::ReportsHealth),
                    })?),
                    None => None,
                };
                Ok((id.clone(), (opamp_client, runner)))
            })
            .collect();

        match agent_runners {
            Err(e) => Err(e),
            Ok(agent_runners) => Ok(SupervisorGroup(agent_runners)),
        }
    }

    pub fn run(self) -> SupervisorGroup<C::Handle, Running> {
        let running = self
            .0
            .into_iter()
            .map(|(t, (opamp, runners))| {
                let client = match opamp {
                    Some(client) => Some(block_on(client.start()).unwrap()),
                    None => None,
                };

                let mut running_runners = Vec::new();

                for runner in runners {
                    running_runners.push(runner.run());
                }
                (t, (client, running_runners))
            })
            .collect();
        SupervisorGroup(running)
    }
}

impl<C> SupervisorGroup<C, Running>
where
    C: OpAMPClientHandle,
{
    pub fn stop(self) -> HashMap<AgentID, Vec<JoinHandle<()>>> {
        self.0
            .into_iter()
            .map(|(t, (opamp, runners))| {
                // stop the OpAMP client
                let _client = match opamp {
                    Some(client) => Some(block_on(client.stop()).unwrap()),
                    None => None,
                };

                let mut stopped_runners = Vec::new();
                for runner in runners {
                    stopped_runners.push(runner.stop());
                }
                (t, stopped_runners)
            })
            .collect()
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
//     use opamp_client::operation::capabilities::Capabilities;
//
//     use super::*;
//
//     use std::{collections::HashMap, sync::mpsc::Sender};
//
//     use crate::agent::error::AgentError;
//     use crate::agent::instance_id::test::MockInstanceIDGetterMock;
//     use crate::config::agent_type::RuntimeConfig;
//     use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
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
//     pub fn new_sleep_supervisor_group<B: OpAMPClientBuilder>(
//         tx: Sender<Event>,
//         builder: B,
//     ) -> Result<SupervisorGroup<Option<B::Client>, Stopped>, AgentError> {
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
//                     Some(builder
//                         .build(StartSettings {
//                             instance_id: "testing".to_string(),
//                             capabilities: Capabilities::default(),
//                         })
//                         .unwrap()),
//                     vec![
//                         new_sleep_supervisor(tx.clone(), 10),
//                         new_sleep_supervisor(tx.clone(), 10),
//                     ],
//                 ),
//             ),>
//         ]);
//         Ok(SupervisorGroup(group))
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
// }
