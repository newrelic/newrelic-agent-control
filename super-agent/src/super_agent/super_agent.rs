use crate::agent_type::definition::AgentType;
use crate::event::channel::{pub_sub, EventConsumer, EventPublisher};
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::remote_config::RemoteConfig;
use crate::opamp::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::opamp::remote_config_report::report_remote_config_status_applied;
use crate::sub_agent::collection::{NotStartedSubAgents, StartedSubAgents};
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::logger::{AgentLog, EventLogger, StdEventReceiver};
use crate::sub_agent::SubAgentBuilder;

use crate::event::{OpAMPEvent, SubAgentEvent, SuperAgentEvent};
use crate::sub_agent::NotStartedSubAgent;
use crate::super_agent::defaults::{SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION};
use crate::super_agent::error::AgentError;
use crate::super_agent::super_agent::EffectiveAgentsError::{
    EffectiveAgentExists, EffectiveAgentNotFound,
};
use crate::utils::time::get_sys_time_nano;
use crossbeam::select;
use opamp_client::StartedClient;
use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use super::config::{
    AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig, SuperAgentConfigError,
};
use super::opamp::remote_config_publisher::SuperAgentRemoteConfigPublisher;
use super::store::{
    SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer, SuperAgentConfigStoreFile,
};

pub type SuperAgentCallbacks = AgentCallbacks<SuperAgentRemoteConfigPublisher>;

pub struct SuperAgent<'a, S, O, HR = HashRepositoryFile, SL = SuperAgentConfigStoreFile>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
    S: SubAgentBuilder,
{
    pub(super) opamp_client: &'a Option<O>,
    sub_agent_builder: S,
    remote_config_hash_repository: &'a HR,
    agent_id: AgentID,
    pub(super) sub_agents_config_store: Arc<SL>,
}

impl<'a, S, O, HR, SL> SuperAgent<'a, S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
{
    pub fn new(
        opamp_client: &'a Option<O>,
        remote_config_hash_repository: &'a HR,
        sub_agent_builder: S,
        sub_agents_config_store: Arc<SL>,
    ) -> Self {
        Self {
            opamp_client,
            remote_config_hash_repository,
            sub_agent_builder,
            // unwrap as we control content of the SUPER_AGENT_ID constant
            agent_id: AgentID::new_super_agent_id(),
            sub_agents_config_store,
        }
    }

    pub fn run(
        self,
        super_agent_consumer: EventConsumer<SuperAgentEvent>,
        opamp_pub_sub: (EventPublisher<OpAMPEvent>, EventConsumer<OpAMPEvent>),
    ) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        // Channel will be closed when tx is dropped and no reference to it is alive
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        if let Some(opamp_handle) = self.opamp_client {
            // TODO should we error on first launch with no hash file?
            let remote_config_hash = self
                .remote_config_hash_repository
                .get(&self.agent_id)
                .map_err(|e| {
                    warn!(
                        "OpAMP enabled but no previous remote configuration found: {}",
                        e
                    )
                })
                .ok();

            if let Some(mut hash) = remote_config_hash {
                if !hash.is_applied() {
                    report_remote_config_status_applied(opamp_handle, &hash)?;
                    self.set_config_hash_as_applied(&mut hash)?;
                }
            }
        }

        info!("Starting the supervisor group.");
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let sub_agents_config = &self.sub_agents_config_store.load()?;

        let not_started_sub_agents =
            self.load_sub_agents(sub_agents_config, &tx, sub_agent_publisher.clone())?;

        // Run all the Sub Agents
        let running_sub_agents = not_started_sub_agents.run()?;
        if let Some(handle) = self.opamp_client {
            info!("Setting SA status to healthy since agent initialization worked");
            let health = opamp_client::opamp::proto::AgentHealth {
                healthy: true,
                last_error: "".to_string(),
                start_time_unix_nano: get_sys_time_nano()?,
            };
            crate::runtime::tokio_runtime().block_on(handle.set_health(health))?;
        }

        self.process_events(
            super_agent_consumer,
            opamp_pub_sub.1,
            (sub_agent_publisher, sub_agent_consumer),
            running_sub_agents,
            tx,
        )?;

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("SuperAgent finished");
        Ok(())
    }

    pub(super) fn set_config_hash_as_applied(&self, hash: &mut Hash) -> Result<(), AgentError> {
        hash.apply();
        self.remote_config_hash_repository
            .save(&self.agent_id, hash)?;

        Ok(())
    }

    // load_sub_agents returns a collection of not started sub agents given the corresponding
    // EffectiveAgents
    fn load_sub_agents(
        &self,
        sub_agents_config: &SubAgentsConfig,
        tx: &Sender<AgentLog>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<NotStartedSubAgents<S::NotStartedSubAgent>, AgentError> {
        Ok(NotStartedSubAgents::from(
            sub_agents_config
                .agents
                .iter()
                .map(|(agent_id, sub_agent_config)| {
                    // FIXME: we force OK(agent) but we need to check also agent not assembled when
                    // on first stat because it can be a crash after a remote_config_change
                    let not_started_agent = self.sub_agent_builder.build(
                        agent_id.clone(),
                        sub_agent_config,
                        tx.clone(),
                        sub_agent_publisher.clone(),
                    )?;
                    Ok((agent_id.clone(), not_started_agent))
                })
                .collect::<Result<HashMap<_, _>, SubAgentBuilderError>>()?,
        ))
    }

    // Recreates a Sub Agent by its agent_id meaning:
    //  * Remove and stop the existing running Sub Agent from the Running Sub Agents
    //  * Recreate the Final Agent using the Agent Type and the latest persisted config
    //  * Build a Stopped Sub Agent
    //  * Run the Sub Agent and add it to the Running Sub Agents
    pub(super) fn recreate_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_remove(&agent_id)?;

        self.create_sub_agent(
            agent_id,
            sub_agent_config,
            tx,
            running_sub_agents,
            sub_agent_publisher,
        )
    }

    // runs and adds into the sub_agents collection the given agent
    fn create_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        running_sub_agents.insert(
            agent_id.clone(),
            self.sub_agent_builder
                .build(agent_id, sub_agent_config, tx, sub_agent_publisher)?
                .run()?,
        );

        Ok(())
    }

    fn process_events(
        &self,
        super_agent_consumer: EventConsumer<SuperAgentEvent>,
        super_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        sub_agent_pub_sub: (EventPublisher<SubAgentEvent>, EventConsumer<SubAgentEvent>),
        mut sub_agents: StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
    ) -> Result<(), AgentError> {
        loop {
            select! {
                recv(super_agent_opamp_consumer.as_ref()) -> opamp_event => {
                    match opamp_event.unwrap() {
                        OpAMPEvent::InvalidRemoteConfigReceived(
                            remote_config_error,
                        ) => {
                            self.invalid_remote_config(remote_config_error)?
                        }
                        OpAMPEvent::ValidRemoteConfigReceived(remote_config) => {
                            self.valid_remote_config(remote_config, sub_agent_pub_sub.0.clone(), &mut sub_agents, tx.clone())?
                        }
                    }

                },
                recv(super_agent_consumer.as_ref()) -> _super_agent_event => {
                        drop(tx); //drop the main channel sender to stop listener
                        break sub_agents.stop()?;
                },
                recv(sub_agent_pub_sub.1.as_ref()) -> sub_agent_event_res => {
                    match sub_agent_event_res {
                        Err(_) => {
                            // TODO is it worth to log this?
                            debug!("channel closed");
                        },
                        Ok(sub_agent_event) => {
                            match sub_agent_event{
                                SubAgentEvent::ConfigUpdated(agent_id) => {
                                    self.sub_agent_config_updated(agent_id,tx.clone(),sub_agent_pub_sub.0.clone(),&mut sub_agents)?
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // apply a remote config to the running sub agents
    pub(super) fn apply_remote_config(
        &self,
        remote_config: RemoteConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        //TODO fix get_unique to fit OpAMP Spec of having a "" when single config
        let content = remote_config.get_unique()?;
        let old_sub_agents_config = self.sub_agents_config_store.load()?;

        let sub_agents_config = if !content.is_empty() {
            SubAgentsConfig::try_from(&remote_config)?
        } else {
            self.sub_agents_config_store.delete()?;
            self.sub_agents_config_store.load()?
        };

        // recreate from new configuration
        sub_agents_config
            .iter()
            .try_for_each(|(agent_id, agent_config)| {
                if let Ok(old_sub_agent_config) = old_sub_agents_config.get(agent_id) {
                    if old_sub_agent_config != agent_config {
                        // new config
                        info!("Recreating SubAgent {}", agent_id);
                        return self.recreate_sub_agent(
                            agent_id.clone(),
                            agent_config,
                            tx.clone(),
                            running_sub_agents,
                            sub_agent_publisher.clone(),
                        );
                    } else {
                        // no changes applied
                        return Ok(());
                    }
                }
                info!("Creating SubAgent {}", agent_id);
                self.create_sub_agent(
                    agent_id.clone(),
                    agent_config,
                    tx.clone(),
                    running_sub_agents,
                    sub_agent_publisher.clone(),
                )
            })?;

        // remove sub agents not used anymore
        old_sub_agents_config
            .iter()
            .try_for_each(|(agent_id, _agent_config)| {
                if let Err(SuperAgentConfigError::SubAgentNotFound(_)) =
                    sub_agents_config.get(agent_id)
                {
                    info!("Stopping SubAgent {}", agent_id);
                    return running_sub_agents.stop_remove(agent_id);
                }
                Ok(())
            })?;

        // TODO improve this code.
        if !content.is_empty() {
            self.sub_agents_config_store.store(&sub_agents_config)?;
        }
        //
        Ok(self
            .remote_config_hash_repository
            .save(&self.agent_id, &remote_config.hash)?)
    }
}

pub fn super_agent_fqn() -> AgentTypeFQN {
    AgentTypeFQN::from(
        format!(
            "{}/{}:{}",
            SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION
        )
        .as_str(),
    )
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct EffectiveAgents {
    pub agents: HashMap<AgentID, AgentType>,
}

#[derive(Error, Debug)]
pub enum EffectiveAgentsError {
    #[error("effective agent `{0}` not found")]
    EffectiveAgentNotFound(String),
    #[error("effective agent `{0}` already exists")]
    EffectiveAgentExists(String),
}

impl EffectiveAgents {
    pub fn get(&self, agent_id: &AgentID) -> Result<&AgentType, EffectiveAgentsError> {
        match self.agents.get(agent_id) {
            None => Err(EffectiveAgentNotFound(agent_id.to_string())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(&mut self, agent_id: AgentID, agent: AgentType) -> Result<(), EffectiveAgentsError> {
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
    use crate::event::channel::pub_sub;
    use crate::event::{OpAMPEvent, SubAgentEvent, SuperAgentEvent};
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::test::MockHashRepositoryMock;
    use crate::opamp::remote_config_hash::{Hash, HashRepository};
    use crate::sub_agent::{test::MockSubAgentBuilderMock, SubAgentBuilder};
    use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig};
    use crate::super_agent::store::tests::MockSubAgentsConfigStore;
    use crate::super_agent::store::{
        SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
    };
    use crate::super_agent::super_agent::SuperAgent;
    use mockall::predicate;

    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::test::{MockNotStartedSubAgent, MockStartedSubAgent};
    use opamp_client::StartedClient;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::SuperAgentCallbacks;

    ////////////////////////////////////////////////////////////////////////////////////
    // Custom Agent constructor for tests
    ////////////////////////////////////////////////////////////////////////////////////
    impl<'a, S, O, HR, SL> SuperAgent<'a, S, O, HR, SL>
    where
        O: StartedClient<SuperAgentCallbacks>,
        HR: HashRepository,
        S: SubAgentBuilder,
        SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
    {
        pub fn new_custom(
            opamp_client: &'a Option<O>,
            remote_config_hash_repository: &'a HR,
            sub_agent_builder: S,
            sub_agents_config_store: SL,
        ) -> Self {
            SuperAgent {
                opamp_client,
                remote_config_hash_repository,
                sub_agent_builder,
                agent_id: AgentID::new_super_agent_id(),
                sub_agents_config_store: Arc::new(sub_agents_config_store),
            }
        }
    }

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(HashMap::new().into()));

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        let opamp_client = Some(started_client);
        // no agents in the supervisor group
        let agent = SuperAgent::new_custom(
            &opamp_client,
            &hash_repository_mock,
            MockSubAgentBuilderMock::new(),
            sub_agents_config_store,
        );

        let (super_agent_publisher, super_agent_consumer) = pub_sub();

        super_agent_publisher
            .publish(SuperAgentEvent::StopRequested)
            .unwrap();

        assert!(agent.run(super_agent_consumer, pub_sub()).is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        let sub_agents_config = sub_agents_default_config();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);

        sub_agents_config_store
            .expect_load()
            .returning(move || Ok(sub_agents_config.clone()));

        let opamp_client = Some(started_client);
        let agent = SuperAgent::new_custom(
            &opamp_client,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
        );

        let (super_agent_publisher, super_agent_consumer) = pub_sub();

        super_agent_publisher
            .publish(SuperAgentEvent::StopRequested)
            .unwrap();

        assert!(agent.run(super_agent_consumer, pub_sub()).is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(sub_agents_default_config()));
        // updated agent
        sub_agents_config_store
            .expect_store()
            .once()
            .returning(|_| Ok(()));

        hash_repository_mock
            .expect_get()
            .with(predicate::eq(AgentID::new_super_agent_id()))
            .times(1)
            .returning(|_| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.apply();
                Ok(hash)
            });

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_super_agent_id()),
                predicate::eq(Hash::new("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_super_agent_id()),
                predicate::eq(Hash::applied("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);

        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();

        let running_agent = spawn({
            let opamp_publisher = opamp_publisher.clone();
            move || {
                // two agents in the supervisor group
                let opamp_client = Some(started_client);
                let agent = SuperAgent::new_custom(
                    &opamp_client,
                    &hash_repository_mock,
                    sub_agent_builder,
                    sub_agents_config_store,
                );
                agent.run(super_agent_consumer, (opamp_publisher, opamp_consumer))
            }
        });

        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.1"
"#
                .to_string(),
            )])),
        };

        opamp_publisher
            .publish(OpAMPEvent::ValidRemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(500));
        super_agent_publisher
            .publish(SuperAgentEvent::StopRequested)
            .unwrap();

        assert!(running_agent.join().is_ok())
    }

    #[test]
    fn create_stop_sub_agents_from_remote_config() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config();

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three times (2 + 1 + 1)
        sub_agent_builder.should_build(3);

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        // all agents on first load
        sub_agents_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        sub_agents_config_store
            .expect_load()
            .once()
            .return_once(|| {
                Ok(HashMap::from([(
                    AgentID::new("nrdot").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                    },
                )])
                .into())
            });

        sub_agents_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        sub_agents_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.should_save_hash(
            &AgentID::new_super_agent_id(),
            &Hash::new("a-hash".to_string()),
        );
        hash_repository_mock.should_save_hash(
            &AgentID::new_super_agent_id(),
            &Hash::new("b-hash".to_string()),
        );

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            &None::<MockStartedOpAMPClientMock<SuperAgentCallbacks>>,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
        );

        let (tx, _) = mpsc::channel();

        let (opamp_publisher, _opamp_consumer) = pub_sub();

        let sub_agents =
            super_agent.load_sub_agents(&sub_agents_config, &tx, opamp_publisher.clone());

        let mut running_sub_agents = sub_agents.unwrap().run().unwrap();

        // just one agent, it should remove the infra-agent
        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#
                .to_string(),
            )])),
        };

        assert_eq!(running_sub_agents.len(), 2);

        super_agent
            .apply_remote_config(
                remote_config,
                tx.clone(),
                &mut running_sub_agents,
                opamp_publisher.clone(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("b-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.0.1
"#
                .to_string(),
            )])),
        };

        super_agent
            .apply_remote_config(
                remote_config,
                tx,
                &mut running_sub_agents,
                opamp_publisher.clone(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        assert!(running_sub_agents.stop().is_ok())
    }

    #[test]
    fn test_sub_agent_config_updated_should_recreate_sub_agent() {
        let (tx, _) = std::sync::mpsc::channel();
        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();

        // Given that we have 3 running Sub Agents
        let sub_agent_id = AgentID::new("infra-agent").unwrap();
        let mut sub_agents = StartedSubAgents::from(HashMap::from([
            (
                AgentID::new("fluent-bit").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (sub_agent_id.clone(), MockStartedSubAgent::new()),
            (AgentID::new("nrdot").unwrap(), MockStartedSubAgent::new()),
        ]));

        let sub_agents_config = SubAgentsConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_rdot"),
                },
            ),
            (
                sub_agent_id.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_infra_agent"),
                },
            ),
            (
                AgentID::new("fluent-bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_fluent_bit"),
                },
            ),
        ]));

        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get(&sub_agent_id).should_stop();
        // And the Sub Agent should be re-created
        let mut not_started_sub_agent = MockNotStartedSubAgent::default();
        // and it will be started
        let mut started_sub_agent = MockStartedSubAgent::default();
        // and will be stopped in the end
        started_sub_agent.should_stop();

        not_started_sub_agent.should_run(started_sub_agent);

        sub_agent_builder.should_build_not_started(
            &sub_agent_id,
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("fqn_infra_agent"),
            },
            not_started_sub_agent,
        );
        // And all the Sub Agents should stop on Stopping the Super Agent
        sub_agents
            .get(&AgentID::new("nrdot").unwrap())
            .should_stop();
        sub_agents
            .get(&AgentID::new("fluent-bit").unwrap())
            .should_stop();

        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (_super_agent_opamp_publisher, super_agent_opamp_consumer) = pub_sub();

        let opamp_client = Some(MockStartedOpAMPClientMock::new());
        // Create the Super Agent and run Sub Agents
        let super_agent = SuperAgent::new_custom(
            &opamp_client,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
        );

        let sub_agent_publisher_clone = sub_agent_publisher.clone();
        let super_agent_publisher_clone = super_agent_publisher.clone();
        spawn(move || {
            sleep(Duration::from_millis(20));

            sub_agent_publisher_clone
                .publish(SubAgentEvent::ConfigUpdated(
                    AgentID::new("infra-agent").unwrap(),
                ))
                .unwrap();

            super_agent_publisher_clone
                .publish(SuperAgentEvent::StopRequested)
                .unwrap();
        });

        super_agent
            .process_events(
                super_agent_consumer,
                super_agent_opamp_consumer,
                (sub_agent_publisher, sub_agent_consumer),
                sub_agents,
                tx,
            )
            .unwrap();
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////

    fn sub_agents_default_config() -> SubAgentsConfig {
        HashMap::from([
            (
                AgentID::new("infra-agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                    ),
                },
            ),
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                },
            ),
        ])
        .into()
    }
}
