use std::time::{SystemTime, SystemTimeError};
use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::{AgentCapabilities, AgentHealth};
use opamp_client::operation::settings::{AgentDescription, StartSettings};
use opamp_client::{capabilities, OpAMPClient, OpAMPClientHandle};
use thiserror::Error;
use tracing::info;

use crate::agent::instance_id::InstanceIDGetter;
use crate::config::agent_configs::AgentTypeFQN;
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

use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};

fn get_sys_time_nano() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos() as u64)
}

#[derive(Error, Debug)]
pub enum SupervisorGroupError {
    #[error("`{0}`")]
    AgentRepositoryError(#[from] AgentRepositoryError),
    #[error("no on_host deployment configuration provided")]
    OnHostDeploymentNotFound,
    #[error("`{0}`")]
    OpAMPBuilderError(#[from] OpAMPClientBuilderError),
    #[error("`{0}`")]
    OpAMPClientError(String),
    #[error("`{0}`")]
    SystemTimeError(#[from] SystemTimeError),
}

struct AgentRunner<C, S> {
    opamp_connection: Option<C>,
    runners: Vec<SupervisorRunner<S>>,
}

#[derive(Default)]
pub struct SupervisorGroup<C, S>(HashMap<AgentID, AgentRunner<C, S>>);

impl<C, S> AgentRunner<C, S> {
    fn new(opamp_connection: Option<C>, runners: Vec<SupervisorRunner<S>>) -> Self {
        Self {
            opamp_connection,
            runners,
        }
    }
}

impl<C> SupervisorGroup<C, Stopped>
where
    C: OpAMPClient,
{
    pub fn new<Repo, OpAMPBuilder, ID>(
        effective_agent_repository: &Repo,
        tx: Sender<Event>,
        cfg: SuperAgentConfig,
        opamp_builder: Option<&OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, SupervisorGroupError>
    where
        Repo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        let agent_runners = cfg
            .agents
            .iter()
            .map(|(agent_t, agent_supervisor_config)| {
                let agent = effective_agent_repository.get(&agent_t.clone().get())?;

                let on_host = agent
                    .runtime_config
                    .deployment
                    .on_host
                    .clone()
                    .ok_or(SupervisorGroupError::OnHostDeploymentNotFound)?;

                let (id, runner) = build_on_host_runners(&tx, agent_t, on_host);
                let opamp_client = match &opamp_builder {
                    Some(builder) => Some(builder.build(start_settings(
                        instance_id_getter.get(id.clone().get()),
                        &agent_supervisor_config.agent_type,
                    ))?),
                    None => None,
                };
                Ok((id.clone(), AgentRunner::new(opamp_client, runner)))
            })
            .collect();

        match agent_runners {
            Err(e) => Err(e),
            Ok(agent_runners) => Ok(SupervisorGroup(agent_runners)),
        }
    }

    pub fn run(self) -> Result<SupervisorGroup<C::Handle, Running>, SupervisorGroupError> {
        let running: Result<
            HashMap<AgentID, AgentRunner<C::Handle, Running>>,
            SupervisorGroupError,
        > = self
            .0
            .into_iter()
            .map(|(t, agent)| {
                let client = match agent.opamp_connection {
                    Some(client) => {
                        info!(
                            "Starting OpAMP client for supervised agent type: Running{}",
                            t
                        );
                        // start the OpAMP client
                        let mut handle = block_on(client.start()).map_err(|err| {
                            SupervisorGroupError::OpAMPClientError(err.to_string())
                        })?;
                        // set OpAMP health
                        block_on(handle.set_health(&AgentHealth {
                            healthy: true,
                            start_time_unix_nano: get_sys_time_nano()?,
                            last_error: "".to_string(),
                        }))
                        .map_err(|err| SupervisorGroupError::OpAMPClientError(err.to_string()))?;
                        Some(handle)
                    }
                    None => None,
                };

                let mut running_runners = Vec::new();

                for runner in agent.runners {
                    running_runners.push(runner.run());
                }
                Ok((t, AgentRunner::new(client, running_runners)))
            })
            .collect();
        Ok(SupervisorGroup(running?))
    }
}

fn start_settings(instance_id: String, agent_fqn: &AgentTypeFQN) -> StartSettings {
    StartSettings {
        instance_id,
        capabilities: capabilities!(AgentCapabilities::ReportsHealth),
        agent_description: AgentDescription {
            identifying_attributes: HashMap::from([
                ("service.name".to_string(), agent_fqn.name().into()),
                (
                    "service.namespace".to_string(),
                    agent_fqn.namespace().into(),
                ),
                ("service.version".to_string(), agent_fqn.version().into()),
            ]),
            non_identifying_attributes: HashMap::from([(
                "host.name".to_string(),
                get_hostname().into(),
            )]),
        },
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}

impl<C> SupervisorGroup<C, Running>
where
    C: OpAMPClientHandle,
{
    pub fn stop(self) -> Result<HashMap<AgentID, Vec<JoinHandle<()>>>, SupervisorGroupError> {
        self.0
            .into_iter()
            .map(|(t, agent)| {
                // stop the OpAMP client
                let _client = match agent.opamp_connection {
                    Some(mut client) => {
                        info!("Stopping OpAMP client for supervised agent type: {}", t);
                        // set OpAMP health
                        block_on(client.set_health(&AgentHealth {
                            healthy: false,
                            start_time_unix_nano: get_sys_time_nano()?,
                            last_error: "".to_string(),
                        }))
                        .map_err(|err| SupervisorGroupError::OpAMPClientError(err.to_string()))?;

                        Some(block_on(client.stop()).map_err(|err| {
                            SupervisorGroupError::OpAMPClientError(err.to_string())
                        })?)
                    }
                    None => None,
                };

                let mut stopped_runners = Vec::new();
                for runner in agent.runners {
                    stopped_runners.push(runner.stop());
                }
                Ok((t, stopped_runners))
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

#[cfg(test)]
pub mod tests {

    use super::*;

    use std::{collections::HashMap, sync::mpsc::Sender};

    use crate::agent::error::AgentError;
    use crate::agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::config::agent_type::RuntimeConfig;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::{
        command::stream::Event,
        config::agent_configs::{AgentID, AgentSupervisorConfig, SuperAgentConfig},
        config::agent_type::{Agent, Deployment, Executable, OnHost},
        config::agent_type_registry::{AgentRepository, LocalRepository},
        supervisor::runner::{sleep_supervisor_tests::new_sleep_supervisor, Stopped},
    };

    // new_sleep_supervisor_group returns a stopped supervisor group with 2 runners with
    // generic agents one with one exec and the other with 2
    pub fn new_sleep_supervisor_group<B: OpAMPClientBuilder>(
        tx: Sender<Event>,
        builder: Option<&B>,
    ) -> Result<SupervisorGroup<B::Client, Stopped>, AgentError> {
        let group: HashMap<AgentID, AgentRunner<B::Client, Stopped>> = HashMap::from([
            (
                AgentID("sleep_5".to_string()),
                AgentRunner::new(
                    builder.map(|b| {
                        b.build(StartSettings {
                            instance_id: "testing".to_string(),
                            ..Default::default()
                        })
                        .unwrap()
                    }),
                    vec![new_sleep_supervisor(tx.clone(), 5)],
                ),
            ),
            (
                AgentID("sleep_10".to_string()),
                AgentRunner::new(
                    builder.map(|b| {
                        b.build(StartSettings {
                            instance_id: "testing".to_string(),
                            ..Default::default()
                        })
                        .unwrap()
                    }),
                    vec![
                        new_sleep_supervisor(tx.clone(), 10),
                        new_sleep_supervisor(tx.clone(), 10),
                    ],
                ),
            ),
        ]);
        Ok(SupervisorGroup(group))
    }

    #[test]
    fn run_stop_without_opamp() {
        let (tx, _) = std::sync::mpsc::channel();
        let supervisor_group =
            new_sleep_supervisor_group::<MockOpAMPClientBuilderMock>(tx, None).unwrap();
        let running_supervisor_group = supervisor_group.run();
        assert!(
            running_supervisor_group.is_ok(),
            "unable to run supervisor_group without opamp"
        );
        assert!(
            running_supervisor_group.unwrap().stop().is_ok(),
            "unable to stop supervisor_group without opamp"
        );
    }

    #[test]
    fn run_stop_with_opamp() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        // the sleep supervisor group contains two agents running, each one should do the following
        // OpAMP client calls in the supervisor_group methods:
        // - run: start + set_health
        // - stop: stop + set_health
        opamp_builder.expect_build().times(2).returning(|_| {
            let mut opamp_client = MockOpAMPClientMock::new();
            opamp_client.expect_start().with().once().returning(|| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.expect_stop().once().returning(|| Ok(()));
                started_client
                    .expect_set_health()
                    .times(2)
                    .returning(|_| Ok(()));
                Ok(started_client)
            });

            Ok(opamp_client)
        });

        let (tx, _) = std::sync::mpsc::channel();

        let supervisor_group = new_sleep_supervisor_group(tx, Some(&opamp_builder)).unwrap();
        let running_supervisor_group = supervisor_group.run();
        assert!(
            running_supervisor_group.is_ok(),
            "unable to run supervisor_group with opamp"
        );
        assert!(
            running_supervisor_group.unwrap().stop().is_ok(),
            "unable to stop supervisor_group with opamp"
        );
    }

    #[test]
    fn new_supervisor_group_with_opamp_builder() {
        let (tx, _) = std::sync::mpsc::channel();
        let agent_config = SuperAgentConfig {
            agents: HashMap::from([(
                AgentID("agent".to_string()),
                AgentSupervisorConfig {
                    agent_type: "".into(),
                    values_file: "".to_string(),
                },
            )]),
            opamp: Some(crate::config::agent_configs::OpAMPClientConfig::default()),
        };

        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        opamp_builder
            .expect_build()
            .once()
            .return_once(|_| Ok(MockOpAMPClientMock::new()));

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(1)
            .returning(|name| name);

        let mut repository = LocalRepository::default();

        let supervisor_group = SupervisorGroup::<
            <MockOpAMPClientBuilderMock as OpAMPClientBuilder>::Client,
            Stopped,
        >::new(
            &repository,
            tx.clone(),
            agent_config.clone(),
            Some(&opamp_builder),
            &instance_id_getter,
        );

        // Case with no valid key
        assert!(supervisor_group.is_err());

        // Case with valid key but not value
        repository
            .store_with_key(
                "agent".to_string(),
                Agent {
                    metadata: Default::default(),
                    variables: Default::default(),
                    runtime_config: Default::default(),
                },
            )
            .unwrap();
        let supervisor_group = SupervisorGroup::<
            <MockOpAMPClientBuilderMock as OpAMPClientBuilder>::Client,
            Stopped,
        >::new(
            &repository,
            tx.clone(),
            agent_config.clone(),
            Some(&opamp_builder),
            &instance_id_getter,
        );

        assert!(supervisor_group.is_err());

        // Valid case with valid full data
        let mut repository = LocalRepository::default();
        _ = repository.store_with_key(
            "agent".to_string(),
            Agent {
                metadata: Default::default(),
                variables: Default::default(),
                runtime_config: RuntimeConfig {
                    deployment: Deployment {
                        on_host: Some(OnHost {
                            executables: vec![Executable {
                                path: "a-path".to_string(),
                                args: Default::default(),
                                env: Default::default(),
                            }],
                            restart_policy: Default::default(),
                        }),
                    },
                },
            },
        );

        let supervisor_group = SupervisorGroup::<
            <MockOpAMPClientBuilderMock as OpAMPClientBuilder>::Client,
            Stopped,
        >::new(
            &repository,
            tx,
            agent_config.clone(),
            Some(&opamp_builder),
            &instance_id_getter,
        );
        assert_eq!(supervisor_group.unwrap().0.len(), 1)
    }

    #[test]
    fn test_start_settings() {
        let instance_id = "test-instance".to_string();
        let agent_fqn = AgentTypeFQN::from("test-namespace/test-agent-type:1.0.0");

        let settings = start_settings(instance_id.clone(), &agent_fqn);
        let hostname = get_hostname().into();

        assert_eq!(settings.instance_id, instance_id);
        assert_eq!(
            settings.capabilities,
            capabilities!(AgentCapabilities::ReportsHealth)
        );
        assert_eq!(
            settings.agent_description.identifying_attributes,
            HashMap::from([
                ("service.name".to_string(), "test-agent-type".into()),
                ("service.namespace".to_string(), "test-namespace".into()),
                ("service.version".to_string(), "1.0.0".into())
            ])
        );
        assert_eq!(
            settings.agent_description.non_identifying_attributes,
            HashMap::from([("host.name".to_string(), hostname)])
        );
    }
}
