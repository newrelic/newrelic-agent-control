use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self, Sender};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::{AgentCapabilities, RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use opamp_client::StartedClient;
use opamp_client::{capabilities, Client};
use thiserror::Error;
use tracing::{error, info};

use crate::command::logger::{EventLogger, StdEventReceiver};
use crate::command::stream::Event;
use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::remote_config::{RemoteConfig, RemoteConfigError};
use crate::config::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::config::super_agent_configs::{AgentID, SuperAgentConfig};
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::sub_agent::collection::{NotStartedSubAgents, StartedSubAgents};
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::SubAgentBuilder;
use crate::sub_agent::{error::SubAgentError, NotStartedSubAgent};
use crate::super_agent::defaults::{
    SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
};
use crate::super_agent::effective_agents_assembler::{
    EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
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
    RestartSubAgent(AgentID),
    // stop all supervisors
    Stop,
}

pub struct SuperAgent<
    'a,
    Assembler,
    S,
    OpAMPBuilder = OpAMPHttpBuilder,
    ID = ULIDInstanceIDGetter,
    HR = HashRepositoryFile,
> where
    Assembler: EffectiveAgentsAssembler,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
    S: SubAgentBuilder,
{
    instance_id_getter: &'a ID,
    effective_agents_asssembler: Assembler,
    opamp_client_builder: Option<&'a OpAMPBuilder>,
    sub_agent_builder: S,
    remote_config_hash_repository: HR,
    agent_id: AgentID,
}

impl<'a, Assembler, S, OpAMPBuilder, ID, HR> SuperAgent<'a, Assembler, S, OpAMPBuilder, ID, HR>
where
    Assembler: EffectiveAgentsAssembler,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
    S: SubAgentBuilder,
{
    pub fn new(
        effective_agents_asssembler: Assembler,
        opamp_client_builder: Option<&'a OpAMPBuilder>,
        instance_id_getter: &'a ID,
        remote_config_hash_repository: HR,
        sub_agent_builder: S,
    ) -> Self {
        Self {
            instance_id_getter,
            effective_agents_asssembler,
            opamp_client_builder,
            remote_config_hash_repository,
            sub_agent_builder,
            agent_id: AgentID(SUPER_AGENT_ID.to_string()),
        }
    }

    fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<'a, Assembler, S, OpAMPBuilder, ID, HR> SuperAgent<'a, Assembler, S, OpAMPBuilder, ID, HR>
where
    Assembler: EffectiveAgentsAssembler,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
    S: SubAgentBuilder,
{
    pub fn run(
        self,
        ctx: Context<Option<SuperAgentEvent>>,
        super_agent_config: &SuperAgentConfig,
    ) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        // Channel will be closed when tx is dropped and no reference to it is alive
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        // build and start the Agent's OpAMP client if a builder is provided
        let opamp_client = self.start_super_agent_opamp_client(ctx.clone())?;

        if let Some(opamp_handle) = &opamp_client {
            // TODO should we error on first launch with no hash file?
            let remote_config_hash = self
                .remote_config_hash_repository
                .get(self.agent_id())
                .map_err(|e| error!("hash repository error: {}", e))
                .ok();

            if let Some(hash) = remote_config_hash {
                if !hash.is_applied() {
                    self.set_config_hash_as_applied(opamp_handle, hash)?;
                }
            }
        }

        info!("Starting the supervisor group.");
        let effective_agents = self.load_effective_agents(super_agent_config)?;

        let not_started_sub_agents = self.load_sub_agents(effective_agents, &tx)?;

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
        let mut running_sub_agents = not_started_sub_agents.run()?;
        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        SuperAgentEvent::Stop => {
                            drop(tx); //drop the main channel sender to stop listener
                            break running_sub_agents.stop()?;
                        }
                        SuperAgentEvent::RemoteConfig(remote_config) => {
                            self.on_remote_config(&opamp_client, remote_config)?;
                        }
                        SuperAgentEvent::RestartSubAgent(agent_id) => {
                            self.recreate_sub_agent(
                                agent_id,
                                super_agent_config,
                                tx.clone(),
                                &mut running_sub_agents,
                            )?;
                        }
                    };
                }
                // spurious condvar wake up, loop should continue
            }
        }

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
        opamp_client: &OpAMPBuilder::Client,
        mut hash: Hash,
    ) -> Result<(), AgentError> {
        block_on(opamp_client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.get().into_bytes(),
            status: RemoteConfigStatuses::Applied as i32,
            error_message: "".to_string(),
        }))?;
        hash.apply();
        self.remote_config_hash_repository
            .save(self.agent_id(), &hash)?;
        Ok(())
    }

    // load_sub_agents returns a collection of not started sub agents given the corresponding
    // EffectiveAgents
    fn load_sub_agents(
        &self,
        effective_agents: EffectiveAgents,
        tx: &Sender<Event>,
    ) -> Result<NotStartedSubAgents<S::NotStartedSubAgent>, AgentError> {
        Ok(NotStartedSubAgents::from(
            effective_agents
                .agents
                .into_iter()
                .map(|(id, agent)| {
                    let not_started_agent =
                        self.sub_agent_builder
                            .build(agent, id.clone(), tx.clone())?;
                    Ok((id, not_started_agent))
                })
                .collect::<Result<HashMap<AgentID, S::NotStartedSubAgent>, SubAgentBuilderError>>(
                )?,
        ))
    }

    // Recreates a Sub Agent by its agent_id meaning:
    //  * Remove and stop the existing running Sub Agent from the Running Sub Agents
    //  * Recreate the Final Agent using the Agent Type and the latest persisted config
    //  * Build a Stopped Sub Agent
    //  * Run the Sub Agent and add it to the Running Sub Agents
    fn recreate_sub_agent(
        &self,
        agent_id: AgentID,
        super_agent_config: &SuperAgentConfig,
        tx: Sender<Event>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_remove(&agent_id)?;

        let sub_agent_config = super_agent_config.sub_agent_config(&agent_id)?;
        let final_agent = self
            .effective_agents_asssembler
            .assemble_agent(&agent_id, sub_agent_config)?;

        running_sub_agents.insert(
            agent_id.clone(),
            self.sub_agent_builder
                .build(final_agent, agent_id, tx)?
                .run()?,
        );

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
                    self.agent_id().clone(),
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
            instance_id: self.instance_id_getter.get(self.agent_id()),
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

    fn load_effective_agents(
        &self,
        super_agent_config: &SuperAgentConfig,
    ) -> Result<EffectiveAgents, EffectiveAgentsAssemblerError> {
        self.effective_agents_asssembler
            .assemble_agents(super_agent_config)
    }

    // Super Agent on remote config
    fn on_remote_config(
        &self,
        opamp_client: &Option<<OpAMPBuilder as OpAMPClientBuilder>::Client>,
        remote_config: Result<RemoteConfig, RemoteConfigError>,
    ) -> Result<(), SubAgentError> {
        if let Some(handle) = &opamp_client {
            let mut remote_config_status = RemoteConfigStatus::default();
            match remote_config {
                Ok(config) => {
                    //
                    self.remote_config_hash_repository
                        .save(self.agent_id(), &config.hash)?;

                    remote_config_status.last_remote_config_hash = config.hash.get().into_bytes();
                    remote_config_status.status = RemoteConfigStatuses::Applying as i32;
                }
                Err(config_error) => match config_error {
                    RemoteConfigError::InvalidConfig(hash, error) => {
                        remote_config_status.last_remote_config_hash = hash.into_bytes();
                        remote_config_status.error_message = error;
                        remote_config_status.status = RemoteConfigStatuses::Failed as i32;
                    }
                    _ => {
                        unreachable!("only errors with hash will reach this block")
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
    pub agents: HashMap<AgentID, FinalAgent>,
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
        match self.agents.get(agent_id) {
            None => Err(EffectiveAgentNotFound(agent_id.to_string())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(
        &mut self,
        agent_id: AgentID,
        agent: FinalAgent,
    ) -> Result<(), EffectiveAgentsError> {
        if self.get(&agent_id).is_ok() {
            return Err(EffectiveAgentExists(agent_id.to_string()));
        }
        self.agents.insert(agent_id, agent);
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
    use crate::config::persister::config_persister::PersistError::FileError;
    use crate::config::persister::config_writer_file::WriteError;
    use crate::config::remote_config::{ConfigMap, RemoteConfig};
    use crate::config::remote_config_hash::test::MockHashRepositoryMock;
    use crate::config::remote_config_hash::{Hash, HashRepository};
    use crate::config::super_agent_configs::{
        AgentID, AgentTypeFQN, SuperAgentConfig, SuperAgentSubAgentConfig,
    };
    use crate::context::Context;
    use crate::file_reader::test::MockFileReaderMock;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::opamp::client_builder::OpAMPClientBuilder;
    use crate::sub_agent::{test::MockSubAgentBuilderMock, SubAgentBuilder};
    use crate::super_agent::defaults::{
        SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
    };
    use crate::super_agent::effective_agents_assembler::{
        EffectiveAgentsAssembler, LocalEffectiveAgentsAssembler,
    };
    use crate::super_agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::super_agent::instance_id::InstanceIDGetter;
    use crate::super_agent::super_agent::{SuperAgent, SuperAgentEvent};
    use mockall::predicate;
    use nix::unistd::gethostname;
    use opamp_client::capabilities;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::operation::capabilities::Capabilities;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::io::ErrorKind;
    use std::sync::mpsc;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    ////////////////////////////////////////////////////////////////////////////////////
    // Custom Agent constructor for tests
    ////////////////////////////////////////////////////////////////////////////////////
    impl<'a, Assembler, S, OpAMPBuilder, ID, HR> SuperAgent<'a, Assembler, S, OpAMPBuilder, ID, HR>
    where
        Assembler: EffectiveAgentsAssembler,
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
        HR: HashRepository,
        S: SubAgentBuilder,
    {
        pub fn new_custom(
            instance_id_getter: &'a ID,
            effective_agents_asssembler: Assembler,
            opamp_client_builder: Option<&'a OpAMPBuilder>,
            remote_config_hash_repository: HR,
            sub_agent_builder: S,
        ) -> Self {
            SuperAgent {
                effective_agents_asssembler,
                opamp_client_builder,
                instance_id_getter,
                remote_config_hash_repository,
                sub_agent_builder,
                agent_id: AgentID(SUPER_AGENT_ID.to_string()),
            }
        }
    }

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

        let registry = MockAgentRegistryMock::new();

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            SUPER_AGENT_ID.to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();

        let mut conf_persister = MockConfigurationPersisterMock::new();
        conf_persister.should_delete_all_configs();

        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = SuperAgentConfig {
            opamp: None,
            agents: HashMap::new(),
        };

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // no agents in the supervisor group
        let agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            MockSubAgentBuilderMock::new(),
        );

        let ctx = Context::new();

        // stop all agents after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::Stop,
            Duration::from_millis(50),
        );

        assert!(agent.run(ctx, &super_agent_config).is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let hostname = gethostname().unwrap_or_default().into_string().unwrap();

        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        // Super Agent OpAMP
        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

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

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "super-agent".to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();
        let mut conf_persister = MockConfigurationPersisterMock::new();

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(2);
        conf_persister.should_persist_any_agent_config(2);

        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build two subagents: nrdot + infra_agent
        sub_agent_builder.should_build(2);

        let super_agent_config = super_agent_default_config();

        let agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            sub_agent_builder,
        );

        let ctx = Context::new();
        // stop all agents after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::Stop,
            Duration::from_millis(50),
        );
        assert!(agent.run(ctx, &super_agent_config).is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        // Super Agent OpAMP
        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client
                    .expect_set_remote_config_status()
                    .times(1)
                    .returning(|_| Ok(()));
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

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

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "super-agent".to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();
        let mut conf_persister = MockConfigurationPersisterMock::new();

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(2);
        conf_persister.should_persist_any_agent_config(2);

        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = super_agent_default_config();

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock
            .expect_get()
            .with(predicate::eq(AgentID::new(SUPER_AGENT_ID)))
            .times(1)
            .returning(|_| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.apply();
                Ok(hash)
            });

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new(SUPER_AGENT_ID)),
                predicate::eq(Hash::new("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build two subagents: nrdot + infra_agent
        sub_agent_builder.should_build(2);

        // two agents in the supervisor group
        let agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            sub_agent_builder,
        );

        let ctx = Context::new();
        spawn({
            let ctx = ctx.clone();
            let agent_id = AgentID::new(SUPER_AGENT_ID);
            move || {
                let remote_config = RemoteConfig {
                    agent_id,
                    hash: Hash::new("a-hash".to_string()),
                    config_map: ConfigMap::new(HashMap::from([(
                        "my-config".to_string(),
                        "enable_process_metrics:true".to_string(),
                    )])),
                };
                sleep(Duration::from_millis(100));
                ctx.cancel_all(Some(SuperAgentEvent::RemoteConfig(Ok(remote_config))))
                    .unwrap();
                sleep(Duration::from_millis(50));
                ctx.cancel_all(Some(SuperAgentEvent::Stop)).unwrap();
            }
        });
        assert!(agent.run(ctx, &super_agent_config).is_ok())
    }

    #[test]
    fn reload_sub_agent_config() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        // Super Agent OpAMP
        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

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
            final_infra_agent.clone(),
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "super-agent".to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();
        let mut conf_persister = MockConfigurationPersisterMock::new();

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(2);
        conf_persister.should_persist_any_agent_config(2);

        //Sub Agent reload expectations
        let agent_id_to_restart = AgentID("infra_agent".to_string());
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );
        conf_persister.should_delete_agent_config(1, &agent_id_to_restart, &final_infra_agent);
        conf_persister.should_persist_agent_config(1, &agent_id_to_restart, &final_infra_agent);

        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = super_agent_default_config();

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three subagents (2 + 1 recreation)
        sub_agent_builder.should_build(3);

        // two agents in the supervisor group
        let agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            sub_agent_builder,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::RestartSubAgent(agent_id_to_restart.clone()),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::Stop,
            Duration::from_millis(100),
        );
        assert!(agent.run(ctx, &super_agent_config).is_ok())
    }

    #[test]
    fn reload_sub_agent_config_error_on_assemble_new_config() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        // Super Agent OpAMP no final stop nor health
        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| Ok(MockOpAMPClientMock::new()),
        );

        // Sub Agents
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });

        let mut registry = MockAgentRegistryMock::new();
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "super-agent".to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();
        let mut conf_persister = MockConfigurationPersisterMock::new();

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(1);
        conf_persister.should_persist_any_agent_config(1);

        //Sub Agent reload expectations
        let agent_id_to_restart = AgentID("infra_agent".to_string());
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );

        conf_persister.should_delete_agent_config(1, &agent_id_to_restart, &final_infra_agent);

        //Persister will fail loading new configuration
        let err = FileError(WriteError::ErrorCreatingFile(std::io::Error::from(
            ErrorKind::PermissionDenied,
        )));

        conf_persister.should_not_persist_agent_config(
            1,
            &agent_id_to_restart,
            &final_infra_agent,
            err,
        );

        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let super_agent_config = super_agent_single_agent();

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.should_get_applied_hash(
            AgentID::new(SUPER_AGENT_ID),
            Hash::new("a-hash".to_string()),
        );

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build one subagent: infra_agent
        sub_agent_builder.should_build(1);

        // two agents in the supervisor group
        let agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            sub_agent_builder,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::RestartSubAgent(agent_id_to_restart.clone()),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::Stop,
            Duration::from_millis(100),
        );

        let result = agent.run(ctx, &super_agent_config);
        assert_eq!("effective agents assembler error: `error assembling agents: `file error: `error creating file: `permission denied````".to_string(), result.err().unwrap().to_string());
    }

    #[test]
    fn recreate_agent_no_errors() {
        let agent_id_to_restart = AgentID("infra_agent".to_string());

        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let super_agent_start_settings = super_agent_default_start_settings(&hostname);

        // Super Agent OpAMP
        opamp_builder.should_build_and_start(
            AgentID::new(SUPER_AGENT_ID),
            super_agent_start_settings,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

        let mut conf_persister = MockConfigurationPersisterMock::new();
        let mut registry = MockAgentRegistryMock::new();
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "super-agent".to_string(),
            "super_agent_instance_id".to_string(),
        );

        let file_reader = MockFileReaderMock::new();

        // Expectations for loading agents
        let mut final_nrdot: FinalAgent = FinalAgent::default();
        final_nrdot.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        registry.should_get(
            "newrelic/io.opentelemetry.collector:0.0.1".to_string(),
            final_nrdot,
        );
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });

        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(2);
        conf_persister.should_persist_any_agent_config(2);

        // Get Infra Agent from registry
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );
        // Clean and persist new config
        conf_persister.should_delete_agent_config(1, &agent_id_to_restart, &final_infra_agent);
        conf_persister.should_persist_agent_config(1, &agent_id_to_restart, &final_infra_agent);

        // Assemble services and Super Agent
        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three sub_agents (2 + 1)
        sub_agent_builder.should_build(3);

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.should_get_applied_hash(
            AgentID::new(SUPER_AGENT_ID),
            Hash::new("a-hash".to_string()),
        );

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            Some(&opamp_builder),
            hash_repository_mock,
            sub_agent_builder,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::RestartSubAgent(agent_id_to_restart.clone()),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::Stop,
            Duration::from_millis(100),
        );

        assert!(super_agent.run(ctx, &super_agent_default_config()).is_ok());
    }

    #[test]
    fn recreate_agent_error_on_persister() {
        let agent_id_to_restart = AgentID("infra_agent".to_string());

        // Mocked services
        let mut conf_persister = MockConfigurationPersisterMock::new();
        let mut registry = MockAgentRegistryMock::new();
        let instance_id_getter = MockInstanceIDGetterMock::new();
        let file_reader = MockFileReaderMock::new();

        // Expectations for loading agents
        let mut final_nrdot: FinalAgent = FinalAgent::default();
        final_nrdot.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        registry.should_get(
            "newrelic/io.opentelemetry.collector:0.0.1".to_string(),
            final_nrdot,
        );
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });

        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );

        conf_persister.should_delete_all_configs();
        conf_persister.should_delete_any_agent_config(2);
        conf_persister.should_persist_any_agent_config(2);

        // Expectations for recreating agent
        // Get Infra Agent from registry
        let mut final_infra_agent: FinalAgent = FinalAgent::default();
        final_infra_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        registry.should_get(
            "newrelic/com.newrelic.infrastructure_agent:0.0.1".to_string(),
            final_infra_agent.clone(),
        );
        // Clean and persist new config
        conf_persister.should_delete_agent_config(1, &agent_id_to_restart, &final_infra_agent);
        //Persister will fail loading new configuration
        let err = FileError(WriteError::ErrorCreatingFile(std::io::Error::from(
            ErrorKind::PermissionDenied,
        )));
        conf_persister.should_not_persist_agent_config(
            1,
            &agent_id_to_restart,
            &final_infra_agent,
            err,
        );

        // Assemble services and Super Agent
        let local_assembler =
            LocalEffectiveAgentsAssembler::new(registry, conf_persister, file_reader);

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build two sub_agents (2 + 0 error)
        sub_agent_builder.should_build(2);

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            &instance_id_getter,
            local_assembler,
            None::<&MockOpAMPClientBuilderMock>,
            MockHashRepositoryMock::new(),
            sub_agent_builder,
        );

        let (tx, _) = mpsc::channel();
        let super_agent_config = super_agent_default_config();
        let effective_agents = super_agent
            .load_effective_agents(&super_agent_config)
            .unwrap();

        let sub_agents = super_agent.load_sub_agents(effective_agents, &tx);

        let mut running_sub_agents = sub_agents.unwrap().run().unwrap();

        let result = super_agent.recreate_sub_agent(
            agent_id_to_restart,
            &super_agent_config,
            tx,
            &mut running_sub_agents,
        );
        assert!(result.is_err());
        assert_eq!(
            "effective agents assembler error: `error assembling agents: `file error: `error creating file: `permission denied````"
                .to_string(),
            result.err().unwrap().to_string()
        );
        assert!(running_sub_agents.stop().is_ok());
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////
    fn super_agent_default_start_settings(hostname: &String) -> StartSettings {
        start_settings(
            "super_agent_instance_id".to_string(),
            capabilities!(AgentCapabilities::ReportsHealth),
            SUPER_AGENT_TYPE.to_string(),
            SUPER_AGENT_VERSION.to_string(),
            SUPER_AGENT_NAMESPACE.to_string(),
            hostname,
        )
    }

    fn start_settings(
        instance_id: String,
        capabilities: Capabilities,
        agent_type: String,
        agent_version: String,
        agent_namespace: String,
        hostname: &String,
    ) -> StartSettings {
        StartSettings {
            instance_id,
            capabilities,
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    ("service.name".to_string(), agent_type.into()),
                    ("service.namespace".to_string(), agent_namespace.into()),
                    ("service.version".to_string(), agent_version.into()),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "host.name".to_string(),
                    DescriptionValueType::String(hostname.clone()),
                )]),
            },
        }
    }

    fn super_agent_default_config() -> SuperAgentConfig {
        SuperAgentConfig {
            opamp: None,
            agents: HashMap::from([
                (
                    AgentID("infra_agent".to_string()),
                    SuperAgentSubAgentConfig {
                        agent_type: AgentTypeFQN::from(
                            "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                        ),
                        values_file: None,
                    },
                ),
                (
                    AgentID("nrdot".to_string()),
                    SuperAgentSubAgentConfig {
                        agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                        values_file: None,
                    },
                ),
            ]),
        }
    }

    fn super_agent_single_agent() -> SuperAgentConfig {
        SuperAgentConfig {
            opamp: None,
            agents: HashMap::from([(
                AgentID("infra_agent".to_string()),
                SuperAgentSubAgentConfig {
                    agent_type: AgentTypeFQN::from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                    ),
                    values_file: None,
                },
            )]),
        }
    }

    fn send_event_after(
        ctx: Context<Option<SuperAgentEvent>>,
        event: SuperAgentEvent,
        after: Duration,
    ) {
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(after);
                ctx.cancel_all(Some(event)).unwrap();
            }
        });
    }
}
