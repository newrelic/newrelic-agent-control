use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self, Sender};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use opamp_client::{capabilities, Client};
use opamp_client::{NotStartedClient, StartedClient};
use thiserror::Error;
use tracing::{error, info};

use crate::agent::defaults::{
    SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
};
use crate::agent::instance_id::{InstanceIDGetter, ULIDInstanceIDGetter};
use crate::agent::EffectiveAgentsError::{EffectiveAgentExists, EffectiveAgentNotFound};
use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::agent_type_registry::AgentRegistry;
use crate::config::persister::config_persister::ConfigurationPersister;
use crate::file_reader::{FSFileReader, FileReader};
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{stream::Event, EventLogger, StdEventReceiver},
    config::{
        agent_configs::{AgentID, SuperAgentConfig},
        supervisor_config::SupervisorConfig,
    },
    context::Context,
    supervisor::runner::Stopped,
};

use self::error::AgentError;

pub mod callbacks;
pub mod defaults;
pub mod error;
pub mod instance_id;
pub mod supervisor_group;

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver<OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agents: &EffectiveAgents,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError>;
}

pub struct Agent<OpAMPBuilder = OpAMPHttpBuilder, ID = ULIDInstanceIDGetter, R = SuperAgentConfig>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    R: SupervisorGroupResolver<OpAMPBuilder, ID>,
{
    resolver: R,
    instance_id_getter: ID,
    effective_agents: EffectiveAgents,
    opamp_client_builder: Option<OpAMPBuilder>,
}

impl<OpAMPBuilder, ID> SupervisorGroupResolver<OpAMPBuilder, ID> for SuperAgentConfig
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agents: &EffectiveAgents,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        Ok(SupervisorGroup::<OpAMPBuilder::Client, Stopped>::new(
            effective_agents,
            tx,
            self.clone(),
            opamp_client_builder.as_ref(),
            instance_id_getter,
        )?)
    }
}

impl<OpAMPBuilder, ID> Agent<OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn new<ConfigPersister: ConfigurationPersister, Registry: AgentRegistry>(
        cfg: SuperAgentConfig,
        agent_type_registry: Registry,
        opamp_client_builder: Option<OpAMPBuilder>,
        instance_id_getter: ID,
        config_persister: &ConfigPersister,
    ) -> Result<Self, AgentError> {
        let reader = FSFileReader;
        let effective_agents =
            load_agent_cfgs(&agent_type_registry, &reader, &cfg, config_persister)?;

        Ok(Self {
            resolver: cfg,
            instance_id_getter,
            effective_agents,
            opamp_client_builder,
        })
    }

    #[cfg(test)]
    pub fn new_custom<R>(
        resolver: R,
        instance_id_getter: ID,
        effective_agents: EffectiveAgents,
        opamp_client_builder: Option<OpAMPBuilder>,
    ) -> Agent<OpAMPBuilder, ID, R>
    where
        R: SupervisorGroupResolver<OpAMPBuilder, ID>,
    {
        Agent {
            resolver,
            effective_agents,
            opamp_client_builder,
            instance_id_getter,
        }
    }
}

impl<OpAMPBuilder, R, ID> Agent<OpAMPBuilder, ID, R>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    R: SupervisorGroupResolver<OpAMPBuilder, ID>,
{
    pub fn run(self, ctx: Context<Option<AgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        // build and start the Agent's OpAMP client if a builder is provided
        let opamp_client_handle = match self.opamp_client_builder {
            Some(ref builder) => {
                info!("Starting superagent's OpAMP Client.");
                let opamp_client = builder.build(StartSettings {
                    instance_id: self.instance_id_getter.get(SUPER_AGENT_ID.to_string()),
                    capabilities: capabilities!(AgentCapabilities::ReportsHealth),
                    agent_description: AgentDescription {
                        identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                            ("service.name".to_string(), SUPER_AGENT_TYPE.into()),
                            (
                                "service.namespace".to_string(),
                                SUPER_AGENT_NAMESPACE.into(),
                            ),
                            ("service.version".to_string(), SUPER_AGENT_VERSION.into()),
                        ]),
                        non_identifying_attributes: HashMap::from([(
                            "host.name".to_string(),
                            gethostname()
                                .unwrap_or_default()
                                .into_string()
                                .unwrap()
                                .into(),
                        )]),
                    },
                })?;
                let opamp_client_handle = block_on(opamp_client.start())?;

                let health = opamp_client::opamp::proto::AgentHealth {
                    healthy: true,
                    last_error: "".to_string(),
                    start_time_unix_nano: 0,
                };
                block_on(opamp_client_handle.set_health(health))?;
                Some(opamp_client_handle)
            }
            None => None,
        };

        info!("Starting the supervisor group.");
        let supervisor_group = self.resolver.retrieve_group(
            tx,
            &self.effective_agents,
            &self.opamp_client_builder,
            &self.instance_id_getter,
        )?;
        /*
            TODO: We should first compare the current config with the one in the super agent config.
            In a future situation, it might have changed due to updates from OpAMP, etc.
            Then, this would require selecting the agents whose config has changed,
            and restarting them.

            FIXME: Given the above comment, this should be converted to a loop in which we modify
            the supervisor group behavior on config changes and selectively restart them as needed.
            For checking the supervisors in a non-blocking way, we can use Handle::is_finished().

            Suppose there's a config change. Situations:
            - Current agents stay as is, new agents are added: start these new agents, merge them with the current group.
            - Current agents stay as is, some agents are removed: get list of these agents (by key), stop and remove them from the current group.
            - Updated config for a certain agent(s) (type, name). Get (by key), stop, remove from the current group, start again with the new config and merge with the running group.

            The "merge" operation can only be done if the agents are of the same type! Supervisor<Running>. If they are not started we won't be able to merge them to the running group, as they are different types.
        */

        // Run all the agents in the supervisor group
        let running_supervisors = supervisor_group.run()?;

        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        AgentEvent::Stop => {
                            break running_supervisors.stop()?.into_iter().for_each(
                                |(agent_id, handles)| {
                                    for handle in handles {
                                        let agent_id = agent_id.clone();
                                        let agent_id1 = agent_id.clone(); // FIXME
                                        handle.join().map_or_else(
                                            |_err| {
                                                // let error: &dyn std::error::Error = &err;
                                                error!(
                                                    supervisor = agent_id.get(),
                                                    msg = "stopped with error",
                                                )
                                            },
                                            |_| {
                                                info!(
                                                    supervisor = agent_id1.get(),
                                                    msg = "stopped successfully"
                                                )
                                            },
                                        )
                                    }
                                },
                            );
                        }

                        AgentEvent::Restart(_agent_type) => {
                            // restart the corresponding supervisor
                            // TODO: remove agent from map, stop, run and reinsert it again
                        }
                    };
                }
                // spurious condvar wake up, loop should continue
            }
        }

        if let Some(handle) = opamp_client_handle {
            info!("Stopping and setting to unhealthy the OpAMP Client");
            let health = opamp_client::opamp::proto::AgentHealth {
                healthy: false,
                last_error: "".to_string(),
                start_time_unix_nano: 0,
            };
            block_on(handle.set_health(health))?;
            block_on(handle.stop())?;
        }

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("SuperAgent finished");
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub struct EffectiveAgents(HashMap<String, FinalAgent>);

#[derive(Error, Debug)]
pub enum EffectiveAgentsError {
    #[error("effective agent `{0}` not found")]
    EffectiveAgentNotFound(String),
    #[error("effective agent `{0}` already exists")]
    EffectiveAgentExists(String),
}

impl EffectiveAgents {
    pub fn default() -> Self {
        EffectiveAgents { 0: HashMap::new() }
    }

    pub fn get(&self, agent_id: &AgentID) -> Result<&FinalAgent, EffectiveAgentsError> {
        match self.0.get(agent_id.get().as_str()) {
            None => Err(EffectiveAgentNotFound(agent_id.get())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(
        &mut self,
        agent_id: &AgentID,
        agent: FinalAgent,
    ) -> Result<(), EffectiveAgentsError> {
        if self.get(agent_id).is_ok() {
            return Err(EffectiveAgentExists(agent_id.get()));
        }
        self.0.insert(agent_id.get().to_string(), agent);
        Ok(())
    }
}

fn load_agent_cfgs<
    Registry: AgentRegistry,
    Reader: FileReader,
    ConfigPersister: ConfigurationPersister,
>(
    agent_registry: &Registry,
    reader: &Reader,
    agent_cfgs: &SuperAgentConfig,
    config_persister: &ConfigPersister,
) -> Result<EffectiveAgents, AgentError> {
    //clean all temporary configurations
    config_persister.clean_all()?;
    let mut effective_agents = EffectiveAgents::default();

    for (agent_id, agent_cfg) in agent_cfgs.agents.iter() {
        //load agent type from repository and populate with values
        let agent_type = agent_registry.get(&agent_cfg.agent_type.to_string())?;
        let mut agent_config: SupervisorConfig = SupervisorConfig::default();
        if let Some(path) = &agent_cfg.values_file {
            let contents = reader.read(path.as_str())?;
            agent_config = serde_yaml::from_str(&contents)?;
        }
        // populate with values
        let populated_agent = agent_type.clone().template_with(agent_config)?;

        // clean existing config files if any
        config_persister.clean(agent_id, &populated_agent)?;

        // persist config if agent requires it
        config_persister.persist(agent_id, &populated_agent)?;

        effective_agents.add(agent_id, populated_agent)?;
    }
    Ok(effective_agents)
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::agent::defaults::{
        SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
    };
    use crate::agent::error::AgentError;
    use crate::agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::agent::instance_id::InstanceIDGetter;
    use crate::agent::{load_agent_cfgs, Agent, AgentEvent, EffectiveAgents};
    use crate::config::agent_configs::{AgentID, AgentSupervisorConfig, SuperAgentConfig};
    use crate::config::agent_type::agent_types::FinalAgent;
    use crate::config::agent_type::trivial_value::TrivialValue;
    use crate::config::agent_type_registry::{AgentRegistry, LocalRegistry};
    use crate::config::persister::config_persister::test::MockConfigurationPersisterMock;
    use crate::config::persister::config_persister::PersistError;
    use crate::config::persister::config_writer_file::WriteError;
    use crate::config::persister::directory_manager::DirectoryManagementError;
    use crate::config::supervisor_config::SupervisorConfig;
    use crate::context::Context;
    use crate::file_reader::test::MockFileReaderMock;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::opamp::client_builder::OpAMPClientBuilder;
    use mockall::predicate;
    use nix::unistd::gethostname;
    use opamp_client::capabilities;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::operation::capabilities::Capabilities;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::{supervisor_group::tests::new_sleep_supervisor_group, SupervisorGroupResolver};

    struct MockedSleepGroupResolver;

    impl<OpAMPBuilder, ID> SupervisorGroupResolver<OpAMPBuilder, ID> for MockedSleepGroupResolver
    where
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agents: &EffectiveAgents,
            opamp_client_builder: &Option<OpAMPBuilder>,
            _instance_id_getter: &ID,
        ) -> Result<
            super::supervisor_group::SupervisorGroup<
                OpAMPBuilder::Client,
                crate::supervisor::runner::Stopped,
            >,
            AgentError,
        > {
            new_sleep_supervisor_group(tx, Some(opamp_client_builder.as_ref().unwrap()))
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let start_settings = StartSettings {
            instance_id: SUPER_AGENT_ID.to_string(),
            capabilities: capabilities!(AgentCapabilities::ReportsHealth),
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    ("service.name".to_string(), SUPER_AGENT_TYPE.into()),
                    (
                        "service.namespace".to_string(),
                        SUPER_AGENT_NAMESPACE.into(),
                    ),
                    ("service.version".to_string(), SUPER_AGENT_VERSION.into()),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "host.name".to_string(),
                    gethostname()
                        .unwrap_or_default()
                        .into_string()
                        .unwrap()
                        .into(),
                )]),
            },
        };

        opamp_builder
            .expect_build()
            .with(predicate::eq(start_settings))
            .times(1)
            .returning(|_| {
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

        let start_settings = StartSettings {
            instance_id: "testing".to_string(),
            capabilities: Capabilities::default(),
            ..Default::default()
        };

        opamp_builder
            .expect_build()
            .with(predicate::eq(start_settings))
            .times(2)
            .returning(|_| {
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

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(1)
            .returning(|name| name);

        // two agents in the supervisor group
        let agent: Agent<
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            MockedSleepGroupResolver,
        > = Agent::new_custom(
            MockedSleepGroupResolver,
            instance_id_getter,
            EffectiveAgents::default(),
            Some(opamp_builder),
        );

        let ctx = Context::new();
        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(3));
                ctx.cancel_all(Some(AgentEvent::Stop)).unwrap();
            }
        });
        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn load_agent_cfgs_test() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();

        file_reader_mock
            .expect_read()
            .with(predicate::eq("second.yaml".to_string()))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_clean_all();

        // cannot assert on agent types as it's iterating a hashmap
        config_persister.should_clean_any(2);
        config_persister.should_persist_any(2);

        let effective_agents = load_agent_cfgs(
            &local_agent_type_repository,
            &file_reader_mock,
            &agent_config,
            &mut config_persister,
        )
        .unwrap();

        let first_agent = effective_agents.get(&AgentID::new("first")).unwrap();
        let file = first_agent
            .variables
            .get("config")
            .unwrap()
            .final_value
            .clone()
            .unwrap();
        let TrivialValue::File(f) = file else {
            unreachable!("Not a file")
        };
        assert_eq!("license_key: abc123\nstaging: true\n", f.content);
    }

    #[test]
    fn load_agent_cfgs_fails_if_cannot_clean_folder() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        //not idempotent test as the order of a hashmap is random
        file_reader_mock.could_read("second.yaml".to_string(), SECOND_TYPE_VALUES.to_string());

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_clean_all();

        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "unauthorized".to_string(),
        ));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_clean_any(err);

        let result = load_agent_cfgs(
            &local_agent_type_repository,
            &file_reader_mock,
            &agent_config,
            &mut config_persister,
        );

        assert_eq!(true, result.is_err());
        assert_eq!("error persisting agent config: `directory error: `cannot delete directory: `unauthorized```".to_string(), result.err().unwrap().to_string());
    }

    #[test]
    fn load_agent_cfgs_fails_if_cannot_write_file() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        file_reader_mock.could_read("second.yaml".to_string(), SECOND_TYPE_VALUES.to_string());

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_clean_all();

        config_persister.should_clean_any(1);
        let err = PersistError::FileError(WriteError::ErrorCreatingFile(Error::from(
            ErrorKind::PermissionDenied,
        )));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_persist_any(err);

        let result = load_agent_cfgs(
            &local_agent_type_repository,
            &file_reader_mock,
            &agent_config,
            &mut config_persister,
        );

        assert_eq!(true, result.is_err());
        assert_eq!("error persisting agent config: `file error: `error creating file: `permission denied```".to_string(), result.err().unwrap().to_string());
    }

    #[test]
    fn empty_load_agent_cfgs_test() {
        let agent_type_registry = LocalRegistry::new();
        let file_reader_mock = MockFileReaderMock::new();
        let agent_config = SuperAgentConfig {
            agents: Default::default(),
            opamp: None,
        };

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_clean_all();

        let effective_agents = load_agent_cfgs(
            &agent_type_registry,
            &file_reader_mock,
            &agent_config,
            &config_persister,
        )
        .unwrap();

        let expected_effective_agents = EffectiveAgents::default();
        assert_eq!(expected_effective_agents, effective_agents);
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Fixtures and helpers
    ////////////////////////////////////////////////////////////////////////////////////

    const FIRST_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  config:
    description: "config file"
    type: file
    required: false
    default: |
        license_key: abc123
        staging: true
    file_path: some_file_name.yml
deployment:
  on_host:
    executables:
      - path: /opt/first
        args: "--config ${config}"
        env: ""
"#;

    const SECOND_TYPE: &str = r#"
namespace: newrelic
name: second
version: 0.1.0
variables:
  deployment:
    on_host:
      path:
        description: "Path to the agent"
        type: string
        required: true
      args:
        description: "Args passed to the agent"
        type: string
        required: false
        default: "an-arg"
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.path}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;

    const SECOND_TYPE_VALUES: &str = r#"
deployment:
  on_host:
    path: another-path
"#;

    // not to copy and paste all the setup of the tests for load_agents_cnf
    fn load_agents_cnf_setup() -> (
        AgentID,
        AgentID,
        LocalRegistry,
        LocalRegistry,
        SuperAgentConfig,
    ) {
        let first_agent_id = AgentID("first".to_string());
        let second_agent_id = AgentID("second".to_string());
        let agent_types_and_values = vec![
            (first_agent_id.clone(), FIRST_TYPE, ""),
            (second_agent_id.clone(), SECOND_TYPE, SECOND_TYPE_VALUES),
        ];

        let mut local_agent_type_repository = LocalRegistry::new();

        // populate "repository" with unpopulated agent types
        agent_types_and_values
            .iter()
            .for_each(|(_, agent_type, _)| {
                let agent_type: FinalAgent =
                    serde_yaml::from_reader(agent_type.as_bytes()).unwrap();
                let res = local_agent_type_repository
                    .store_with_key(agent_type.metadata.to_string(), agent_type);
                assert_eq!(true, res.is_ok());
            });

        // just for the test
        let mut populated_agent_type_repository = LocalRegistry::new();
        // populate "repository" with unpopulated agent types
        agent_types_and_values
            .iter()
            .for_each(|(agent_id, agent_type, agent_values)| {
                let mut agent_type: FinalAgent =
                    serde_yaml::from_reader(agent_type.as_bytes()).unwrap();
                let agent_values: SupervisorConfig =
                    serde_yaml::from_reader(agent_values.as_bytes()).unwrap();
                agent_type = agent_type.template_with(agent_values).unwrap();
                let res = populated_agent_type_repository
                    .store_with_key(agent_id.to_string(), agent_type);

                assert_eq!(true, res.is_ok());
            });

        let agent_config = SuperAgentConfig {
            agents: HashMap::from([
                (
                    first_agent_id.clone(),
                    AgentSupervisorConfig {
                        agent_type: populated_agent_type_repository
                            .get(first_agent_id.get().as_str())
                            .unwrap()
                            .metadata
                            .to_string()
                            .as_str()
                            .into(),
                        values_file: None,
                    },
                ),
                (
                    second_agent_id.clone(),
                    AgentSupervisorConfig {
                        agent_type: populated_agent_type_repository
                            .get(second_agent_id.get().as_str())
                            .unwrap()
                            .metadata
                            .to_string()
                            .as_str()
                            .into(),
                        values_file: Some("second.yaml".to_string()),
                    },
                ),
            ]),
            opamp: None,
        };

        return (
            first_agent_id,
            second_agent_id,
            local_agent_type_repository,
            populated_agent_type_repository,
            agent_config,
        );
    }
}
