use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::{AgentCapabilities, RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use opamp_client::StartedClient;
use opamp_client::{capabilities, Client};
use thiserror::Error;
use tracing::{error, info};

use crate::command::logger::{EventLogger, StdEventReceiver};
use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::remote_config::{RemoteConfig, RemoteConfigError};
use crate::config::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::config::super_agent_configs::AgentID;
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::sub_agent::on_host::factory::build_sub_agents;
use crate::sub_agent::on_host::sub_agents_on_host::StartedSubAgentsOnHost;
use crate::sub_agent::sub_agent::SubAgentError;
use crate::super_agent::defaults::{
    SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
};
use crate::super_agent::error::AgentError;
use crate::super_agent::instance_id::{InstanceIDGetter, ULIDInstanceIDGetter};
use crate::super_agent::super_agent::EffectiveAgentsError::{
    EffectiveAgentExists, EffectiveAgentNotFound,
};

#[derive(Clone)]
pub enum SuperAgentEvent {
    RemoteConfig(Result<RemoteConfig, RemoteConfigError>),
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub struct SuperAgent<
    'a,
    OpAMPBuilder = OpAMPHttpBuilder,
    ID = ULIDInstanceIDGetter,
    HR = HashRepositoryFile,
> where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
{
    instance_id_getter: ID,
    effective_agents: EffectiveAgents,
    opamp_client_builder: Option<&'a OpAMPBuilder>,
    remote_config_hash_repository: HR,
}

impl<'a, OpAMPBuilder, ID, HR> SuperAgent<'a, OpAMPBuilder, ID, HR>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
{
    pub fn new(
        effective_agents: EffectiveAgents,
        opamp_client_builder: Option<&'a OpAMPBuilder>,
        instance_id_getter: ID,
        remote_config_hash_repository: HR,
    ) -> Self {
        Self {
            instance_id_getter,
            effective_agents,
            opamp_client_builder,
            remote_config_hash_repository,
        }
    }
}

impl<'a, OpAMPBuilder, ID, HR> SuperAgent<'a, OpAMPBuilder, ID, HR>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
{
    pub fn run(self, ctx: Context<Option<SuperAgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        // build and start the Agent's OpAMP client if a builder is provided
        let opamp_client = self.start_super_agent_opamp_client(ctx.clone())?;

        let remote_config_hash = self
            .remote_config_hash_repository
            .get(AgentID(SUPER_AGENT_ID.to_string()));

        match remote_config_hash {
            Ok(hash) => {
                if !hash.is_applied() {
                    self.set_config_hash_as_applied(&opamp_client, hash)?;
                }
            }
            Err(e) => {
                error!("hash repository error: {}", e);
            }
        }

        info!("Starting the supervisor group.");
        // create sub agents
        let sub_agents = build_sub_agents(
            &self.effective_agents,
            tx,
            self.opamp_client_builder,
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

        // Run all the Sub Agents
        let running_sub_agents = sub_agents.run()?;
        self.process_event(ctx.clone(), running_sub_agents, &opamp_client)?;

        if let Some(handle) = opamp_client {
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

    fn set_config_hash_as_applied(
        &self,
        opamp_client: &Option<OpAMPBuilder::Client>,
        mut hash: Hash,
    ) -> Result<(), AgentError> {
        if let Some(opamp_handle) = &opamp_client {
            let opamp_result =
                block_on(opamp_handle.set_remote_config_status(RemoteConfigStatus {
                    last_remote_config_hash: hash.get().into_bytes(),
                    status: RemoteConfigStatuses::Applied as i32,
                    error_message: "".to_string(),
                }));
            match opamp_result {
                Ok(_) => {
                    hash.apply();
                    self.remote_config_hash_repository
                        .save(AgentID(SUPER_AGENT_ID.to_string()), hash.clone())?
                }
                Err(e) => return Err(AgentError::from(e)),
            }
        }
        Ok(())
    }

    fn start_super_agent_opamp_client(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Result<Option<OpAMPBuilder::Client>, AgentError> {
        // build and start the Agent's OpAMP client if a builder is provided
        let opamp_client_handle = match self.opamp_client_builder {
            Some(builder) => {
                info!("Starting superagent's OpAMP Client.");
                let opamp_client = builder.build_and_start(
                    ctx,
                    AgentID(SUPER_AGENT_ID.to_string()),
                    self.super_agent_start_settings(),
                )?;
                Some(opamp_client)
            }
            None => None,
        };

        Ok(opamp_client_handle)
    }

    fn super_agent_start_settings(&self) -> StartSettings {
        StartSettings {
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
        }
    }

    fn process_event(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        running_sub_agents: StartedSubAgentsOnHost<<OpAMPBuilder as OpAMPClientBuilder>::Client>,
        opamp_client: &Option<OpAMPBuilder::Client>,
    ) -> Result<(), SubAgentError>
    where
        OpAMPBuilder: OpAMPClientBuilder,
    {
        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        SuperAgentEvent::Stop => {
                            break running_sub_agents.stop()?.into_iter().for_each(
                                |(agent_id, handles)| {
                                    for handle in handles {
                                        let agent_id = agent_id.clone();
                                        let agent_id1 = agent_id.clone(); // FIXME
                                        handle.join().map_or_else(
                                            |_err| {
                                                // let error: &dyn std::error::Error = &err;
                                                error!(
                                                    supervisor = agent_id.to_string(),
                                                    msg = "stopped with error",
                                                )
                                            },
                                            |_| {
                                                info!(
                                                    supervisor = agent_id1.to_string(),
                                                    msg = "stopped successfully"
                                                )
                                            },
                                        )
                                    }
                                },
                            );
                        }

                        SuperAgentEvent::RemoteConfig(remote_config) => {
                            self.on_remote_config(&opamp_client, remote_config)?;
                        }

                        SuperAgentEvent::Restart(_agent_type) => {
                            // restart the corresponding supervisor
                            // TODO: remove agent from map, stop, run and reinsert it again
                        }
                    };
                }
                // spurious condvar wake up, loop should continue
            }
            Ok(())
        }
    }

    fn on_remote_config(&self, opamp_client: &Option<<OpAMPBuilder as OpAMPClientBuilder>::Client>, remote_config: Result<RemoteConfig, RemoteConfigError>) -> Result<(), SubAgentError> {
        if let Some(handle) = &opamp_client {
            let mut remote_config_status = RemoteConfigStatus::default();
            match remote_config {
                Ok(config) => {
                    //
                    self.remote_config_hash_repository
                        .save(config.agent_id, config.hash.clone())?;

                    remote_config_status.last_remote_config_hash =
                        config.hash.get().into_bytes();
                    remote_config_status.status =
                        RemoteConfigStatuses::Applying as i32;
                }
                Err(config_error) => match config_error {
                    RemoteConfigError::UTF8(hash, error) => {
                        remote_config_status.last_remote_config_hash =
                            hash.into_bytes();
                        remote_config_status.error_message = error;
                        remote_config_status.status =
                            RemoteConfigStatuses::Failed as i32;
                    }
                },
            }
            block_on(handle.set_remote_config_status(remote_config_status))?;
        }

        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct EffectiveAgents {
    pub agents: HashMap<String, FinalAgent>,
}

#[derive(Error, Debug)]
pub enum EffectiveAgentsError {
    #[error("effective agent `{0}` not found")]
    EffectiveAgentNotFound(String),
    #[error("effective agent `{0}` already exists")]
    EffectiveAgentExists(String),
}

impl EffectiveAgents {
    pub fn get(&self, agent_id: &AgentID) -> Result<&FinalAgent, EffectiveAgentsError> {
        let agent_id_string = &agent_id.to_string();
        match self.agents.get(agent_id_string) {
            None => Err(EffectiveAgentNotFound(agent_id_string.to_owned())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(
        &mut self,
        agent_id: &AgentID,
        agent: FinalAgent,
    ) -> Result<(), EffectiveAgentsError> {
        if self.get(agent_id).is_ok() {
            return Err(EffectiveAgentExists(agent_id.to_string()));
        }
        self.agents.insert(agent_id.to_string(), agent);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::config::agent_type::agent_types::FinalAgent;
    use crate::config::agent_type::runtime_config::OnHost;
    use crate::config::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::config::persister::config_persister::test::MockConfigurationPersisterMock;
    use crate::config::remote_config::RemoteConfig;
    use crate::config::remote_config_hash::test::MockHashRepositoryMock;
    use crate::config::remote_config_hash::{Hash, HashRepository};
    use crate::config::super_agent_configs::{
        AgentID, AgentTypeFQN, SuperAgentConfig, SuperAgentSubAgentConfig,
    };
    use crate::context::Context;
    use crate::file_reader::test::MockFileReaderMock;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::opamp::client_builder::OpAMPClientBuilder;
    use crate::super_agent::defaults::{
        SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
    };
    use crate::super_agent::effective_agents_assembler::{
        EffectiveAgentsAssembler, LocalEffectiveAgentsAssembler,
    };
    use crate::super_agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::super_agent::instance_id::InstanceIDGetter;
    use crate::super_agent::super_agent::{EffectiveAgents, SuperAgent, SuperAgentEvent};
    use mockall::predicate;
    use nix::unistd::gethostname;
    use opamp_client::capabilities;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::operation::capabilities::Capabilities;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    ////////////////////////////////////////////////////////////////////////////////////
    // Custom Agent constructor for tests
    ////////////////////////////////////////////////////////////////////////////////////
    impl<'a, OpAMPBuilder, ID, HR> SuperAgent<'a, OpAMPBuilder, ID, HR>
    where
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
        HR: HashRepository,
    {
        pub fn new_custom(
            instance_id_getter: ID,
            effective_agents: EffectiveAgents,
            opamp_client_builder: Option<&'a OpAMPBuilder>,
            remote_config_hash_repository: HR,
        ) -> SuperAgent<OpAMPBuilder, ID, HR> {
            SuperAgent {
                effective_agents,
                opamp_client_builder,
                instance_id_getter,
                remote_config_hash_repository,
            }
        }
    }

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let hostname = gethostname().unwrap_or_default().into_string().unwrap();

        let super_agent_start_settings = start_settings(
            SUPER_AGENT_ID.to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            SUPER_AGENT_TYPE.to_string(),
            SUPER_AGENT_VERSION.to_string(),
            SUPER_AGENT_NAMESPACE.to_string(),
            hostname.to_string(),
        );

        // Super Agent OpAMP
        opamp_builder
            .expect_build_and_start()
            .with(
                predicate::always(),
                predicate::eq(AgentID::new(SUPER_AGENT_ID)),
                predicate::eq(super_agent_start_settings),
            )
            .times(1)
            .returning(|_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client
                    .expect_set_health()
                    .times(1)
                    .returning(|_| Ok(()));
                started_client.expect_stop().once().returning(|| Ok(()));
                Ok(started_client)
            });

        let registry = MockAgentRegistryMock::new();

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(1)
            .returning(|name| name);

        let file_reader = MockFileReaderMock::new();

        let mut conf_persister = MockConfigurationPersisterMock::new();
        conf_persister.should_clean_all();

        let mut local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = SuperAgentConfig {
            opamp: None,
            agents: HashMap::new(),
        };

        let effective_agents = local_assembler
            .assemble_agents(&super_agent_config)
            .unwrap();

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // no agents in the supervisor group
        let agent: SuperAgent<
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            MockHashRepositoryMock,
        > = SuperAgent::new_custom(
            instance_id_getter,
            effective_agents,
            Some(&opamp_builder),
            hash_repository_mock,
        );

        let ctx = Context::new();
        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(1));
                ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap();
            }
        });
        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let hostname = gethostname().unwrap_or_default().into_string().unwrap();

        let super_agent_start_settings = start_settings(
            SUPER_AGENT_ID.to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            SUPER_AGENT_TYPE.to_string(),
            SUPER_AGENT_VERSION.to_string(),
            SUPER_AGENT_NAMESPACE.to_string(),
            hostname.to_string(),
        );

        // Super Agent OpAMP
        opamp_builder
            .expect_build_and_start()
            .with(
                predicate::always(),
                predicate::eq(AgentID::new(SUPER_AGENT_ID)),
                predicate::eq(super_agent_start_settings),
            )
            .times(1)
            .returning(|_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client
                    .expect_set_health()
                    .times(1)
                    .returning(|_| Ok(()));
                started_client.expect_stop().once().returning(|| Ok(()));
                Ok(started_client)
            });

        // Sub Agents
        let mut final_nrdot: FinalAgent = FinalAgent::default();
        final_nrdot.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });

        let mut registry = MockAgentRegistryMock::new();
        registry.should_get(
            "newrelic/io.opentelemetry.collector:0.0.1".to_string(),
            final_nrdot,
        );
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent,
        );

        let start_settings_infra = start_settings(
            "infra_agent".to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            hostname.to_string(),
        );

        let start_settings_nrdot = start_settings(
            "nrdot".to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            hostname.to_string(),
        );

        opamp_builder
            .expect_build_and_start()
            .with(
                predicate::always(),
                predicate::eq(AgentID::new("infra_agent")),
                predicate::eq(start_settings_infra),
            )
            .times(1)
            .returning(|_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.expect_stop().once().returning(|| Ok(()));
                started_client
                    .expect_set_health()
                    .times(1)
                    .returning(|_| Ok(()));
                Ok(started_client)
            });

        opamp_builder
            .expect_build_and_start()
            .with(
                predicate::always(),
                predicate::eq(AgentID::new("nrdot")),
                predicate::eq(start_settings_nrdot),
            )
            .times(1)
            .returning(|_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.expect_stop().once().returning(|| Ok(()));
                started_client
                    .expect_set_health()
                    .times(1)
                    .returning(|_| Ok(()));
                Ok(started_client)
            });

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(3)
            .returning(|name| name);

        let file_reader = MockFileReaderMock::new();
        let mut conf_persister = MockConfigurationPersisterMock::new();

        conf_persister.should_clean_all();
        conf_persister.should_clean_any(2);
        conf_persister.should_persist_any(2);

        let mut local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = SuperAgentConfig {
            opamp: None,
            agents: HashMap::from([
                (
                    AgentID::new("infra_agent"),
                    SuperAgentSubAgentConfig {
                        agent_type: AgentTypeFQN::from(
                            "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                        ),
                        values_file: None,
                    },
                ),
                (
                    AgentID::new("nrdot"),
                    SuperAgentSubAgentConfig {
                        agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                        values_file: None,
                    },
                ),
            ]),
        };

        let effective_agents = local_assembler
            .assemble_agents(&super_agent_config)
            .unwrap();

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // no agents in the supervisor group
        let agent: SuperAgent<
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            MockHashRepositoryMock,
        > = SuperAgent::new_custom(
            instance_id_getter,
            effective_agents,
            Some(&opamp_builder),
            hash_repository_mock,
        );

        let ctx = Context::new();
        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(1));
                ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap();
            }
        });
        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let hostname = gethostname().unwrap_or_default().into_string().unwrap();

        let super_agent_start_settings = start_settings(
            SUPER_AGENT_ID.to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            SUPER_AGENT_TYPE.to_string(),
            SUPER_AGENT_VERSION.to_string(),
            SUPER_AGENT_NAMESPACE.to_string(),
            hostname.to_string(),
        );

        // Super Agent OpAMP
        opamp_builder
            .expect_build_and_start()
            .with(
                predicate::always(),
                predicate::eq(AgentID::new(SUPER_AGENT_ID)),
                predicate::eq(super_agent_start_settings),
            )
            .times(1)
            .returning(|_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client
                    .expect_set_health()
                    .times(1)
                    .returning(|_| Ok(()));
                started_client
                    .expect_set_remote_config_status()
                    .times(1)
                    .returning(|_| Ok(()));
                started_client.expect_stop().once().returning(|| Ok(()));
                Ok(started_client)
            });

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(1)
            .returning(|name| name);

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        // two agents in the supervisor group
        let agent: SuperAgent<
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            MockHashRepositoryMock,
        > = SuperAgent::new_custom(
            instance_id_getter,
            EffectiveAgents::default(),
            Some(&opamp_builder),
            hash_repository_mock,
        );

        let ctx = Context::new();
        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(1));
                ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap();
            }
        });

        let ctx = Context::new();
        // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            let agent_id = AgentID::new(SUPER_AGENT_ID);
            move || {
                let remote_config = RemoteConfig {
                    agent_id,
                    hash: Hash::new("a-hash".to_string()),
                    config_map: HashMap::from([(
                        "my-config".to_string(),
                        "enable_process_metrics:true".to_string(),
                    )]),
                };
                sleep(Duration::from_secs(3));
                ctx.cancel_all(Some(SuperAgentEvent::RemoteConfig(Ok(remote_config))))
                    .unwrap();
                sleep(Duration::from_secs(1));
                ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap();
            }
        });
        assert!(agent.run(ctx).is_ok())
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////
    fn start_settings(
        agent_id: String,
        capabilities: Capabilities,
        agent_type: String,
        agent_version: String,
        agent_namespace: String,
        hostname: String,
    ) -> StartSettings {
        StartSettings {
            instance_id: agent_id,
            capabilities: capabilities,
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    ("service.name".to_string(), agent_type.into()),
                    ("service.namespace".to_string(), agent_namespace.into()),
                    ("service.version".to_string(), agent_version.into()),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "host.name".to_string(),
                    hostname.into(),
                )]),
            },
        }
    }
}
