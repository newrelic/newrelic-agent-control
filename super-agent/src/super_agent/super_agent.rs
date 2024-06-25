use super::config::{AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsMap, SuperAgentDynamicConfig};
use super::config_storer::loader_storer::{
    SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader, SuperAgentDynamicConfigStorer,
};
use crate::event::{
    channel::{pub_sub, EventConsumer, EventPublisher},
    ApplicationEvent, OpAMPEvent, SubAgentEvent, SuperAgentEvent,
};
use crate::opamp::{
    callbacks::AgentCallbacks,
    hash_repository::HashRepository,
    remote_config::{RemoteConfig, RemoteConfigError},
    remote_config_hash::Hash,
    remote_config_report::report_remote_config_status_applied,
};
use crate::sub_agent::health::health_checker::{Health, Healthy, Unhealthy};
use crate::sub_agent::{
    collection::{NotStartedSubAgents, StartedSubAgents},
    error::SubAgentBuilderError,
    NotStartedSubAgent, StartedSubAgent, SubAgentBuilder,
};
use crate::super_agent::{
    defaults::{SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION},
    error::AgentError,
};
use crate::utils::time::get_sys_time_nano;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::{opamp::proto::ComponentHealth, StartedClient};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, trace, warn};

pub(super) type SuperAgentCallbacks = AgentCallbacks;

pub struct SuperAgent<S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    SL: SuperAgentDynamicConfigStorer
        + SuperAgentDynamicConfigLoader
        + SuperAgentDynamicConfigDeleter,
    S: SubAgentBuilder,
{
    pub(super) opamp_client: Option<O>,
    sub_agent_builder: S,
    remote_config_hash_repository: Arc<HR>,
    agent_id: AgentID,
    pub(super) sa_dynamic_config_store: Arc<SL>,
    pub(super) super_agent_publisher: EventPublisher<SuperAgentEvent>,
}

impl<S, O, HR, SL> SuperAgent<S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SuperAgentDynamicConfigStorer
        + SuperAgentDynamicConfigLoader
        + SuperAgentDynamicConfigDeleter,
{
    pub fn new(
        opamp_client: Option<O>,
        remote_config_hash_repository: Arc<HR>,
        sub_agent_builder: S,
        sub_agents_config_store: Arc<SL>,
        super_agent_publisher: EventPublisher<SuperAgentEvent>,
    ) -> Self {
        Self {
            opamp_client,
            remote_config_hash_repository,
            sub_agent_builder,
            // unwrap as we control content of the SUPER_AGENT_ID constant
            agent_id: AgentID::new_super_agent_id(),
            sa_dynamic_config_store: sub_agents_config_store,
            super_agent_publisher,
        }
    }

    pub fn run(
        self,
        application_event_consumer: EventConsumer<ApplicationEvent>,
        super_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    ) -> Result<(), AgentError> {
        debug!("Creating agent's communication channels");
        if let Some(opamp_handle) = &self.opamp_client {
            match self.remote_config_hash_repository.get(&self.agent_id) {
                Err(e) => {
                    warn!("Failed getting remote config hash from the store: {}", e);
                }
                Ok(Some(mut hash)) => {
                    if !hash.is_applied() {
                        report_remote_config_status_applied(opamp_handle, &hash)?;
                        self.set_config_hash_as_applied(&mut hash)?;
                    }
                }
                Ok(None) => {
                    warn!("OpAMP enabled but no previous remote configuration found");
                }
            }
        }

        info!("Starting the agents supervisor runtime");
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let sub_agents_config = self.sa_dynamic_config_store.load()?.agents;

        let not_started_sub_agents =
            self.load_sub_agents(&sub_agents_config, sub_agent_publisher.clone())?;

        info!("Agents supervisor runtime successfully started");
        // Run all the Sub Agents
        let running_sub_agents = not_started_sub_agents.run();

        self.process_events(
            application_event_consumer,
            super_agent_opamp_consumer,
            sub_agent_publisher,
            sub_agent_consumer,
            running_sub_agents,
        )?;

        if let Some(handle) = self.opamp_client {
            info!("Stopping the OpAMP Client");
            // We should call disconnect here as this means a graceful shutdown
            handle.stop()?;
        }

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
        sub_agents: &SubAgentsMap,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<NotStartedSubAgents<S::NotStartedSubAgent>, AgentError> {
        Ok(NotStartedSubAgents::from(
            sub_agents
                .iter()
                .map(|(agent_id, sub_agent_config)| {
                    // FIXME: we force OK(agent) but we need to check also agent not assembled when
                    // on first stat because it can be a crash after a remote_config_change
                    let not_started_agent = self.sub_agent_builder.build(
                        agent_id.clone(),
                        sub_agent_config,
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
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_remove(&agent_id)?;

        self.create_sub_agent(
            agent_id,
            sub_agent_config,
            running_sub_agents,
            sub_agent_publisher,
        )
    }

    // runs and adds into the sub_agents collection the given agent
    fn create_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        running_sub_agents.insert(
            agent_id.clone(),
            self.sub_agent_builder
                .build(agent_id, sub_agent_config, sub_agent_publisher)?
                .run(),
        );

        Ok(())
    }

    // process_events listens for events from the Super Agent and the Sub Agents
    // This is the main thread loop, executed after initialization of all Super Agent components.
    fn process_events(
        &self,
        application_event_consumer: EventConsumer<ApplicationEvent>,
        super_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_consumer: EventConsumer<SubAgentEvent>,
        mut sub_agents: StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let _ = self
            .report_healthy(Healthy::new(String::default()))
            .inspect_err(
                |err| error!(error_msg = %err,"Error reporting health on Super Agent start"),
            );

        debug!("Listening for events from agents");
        let never_receive = EventConsumer::from(never());
        let opamp_receiver = super_agent_opamp_consumer
            .as_ref()
            .unwrap_or(&never_receive);
        loop {
            select! {
                recv(&opamp_receiver.as_ref()) -> opamp_event => {
                    match opamp_event.unwrap() {
                        OpAMPEvent::RemoteConfigReceived(remote_config) => {
                            let _ = self.remote_config(remote_config, sub_agent_publisher.clone(), &mut sub_agents )
                            .inspect_err(|err| error!(error_msg = %err,"Error processing valid remote config"));
                        }
                        OpAMPEvent::Connected => {
                            let _ = self.super_agent_publisher
                            .publish(SuperAgentEvent::OpAMPConnected)
                            .inspect_err(|err| error!(error_msg = %err,"cannot publish super_agent_event::super_agent_opamp_connected"));
                        }
                        OpAMPEvent::ConnectFailed(error_code, error_message) => {
                            let _ = self.super_agent_publisher
                            .publish(SuperAgentEvent::OpAMPConnectFailed(error_code, error_message))
                            .inspect_err(|err| error!(error_msg = %err,"cannot publish super_agent_event::super_agent_opamp_connect_failed"));
                        }
                    }
                },
                recv(application_event_consumer.as_ref()) -> _super_agent_event => {
                    debug!("stopping Super Agent event processor");

                    let _ = self.super_agent_publisher
                    .publish(SuperAgentEvent::SuperAgentStopped)
                    .inspect_err(|err| error!(error_msg = %err,"cannot publish super_agent_event::super_agent_stopped"));

                    break sub_agents.stop()?;
                },
                recv(sub_agent_consumer.as_ref()) -> sub_agent_event_res => {
                    debug!("Received SubAgent event");
                    trace!("SubAgent event receive attempt: {:?}", sub_agent_event_res);
                    match sub_agent_event_res {
                        Err(_) => {
                            debug!("channel closed");
                        },
                        Ok(sub_agent_event) => {
                            trace!("SubAgent event: {:?}", sub_agent_event);
                            match sub_agent_event{
                                SubAgentEvent::ConfigUpdated(agent_id) => {
                                    self.sub_agent_config_updated(agent_id,sub_agent_publisher.clone(),&mut sub_agents)?
                                },
                                SubAgentEvent::SubAgentBecameHealthy(agent_id, healthy, start_time) => {
                                    debug!(agent_id = agent_id.to_string() ,"sub agent is healthy");
                                    let Some(sub_agent) = sub_agents.get(&agent_id) else {
                                        error!(agent_id = agent_id.to_string(),"cannot find sub agent on super_agent_event.sub_agent_became_healthy event");
                                        continue;
                                    };

                                    let _ = self.super_agent_publisher
                                    .publish(SuperAgentEvent::SubAgentBecameHealthy(agent_id,sub_agent.agent_type(), healthy, start_time))
                                    .inspect_err(|err| error!(error_msg = %err,"cannot publish super_agent_event.sub_agent_became_healthy"));
                                },
                                SubAgentEvent::SubAgentBecameUnhealthy(agent_id, unhealthy, start_time) => {
                                    debug!(agent_id = agent_id.to_string(), error_message = unhealthy.last_error() ,"sub agent is unhealthy");
                                    let Some(sub_agent) = sub_agents.get(&agent_id) else{
                                        error!(agent_id = agent_id.to_string(),"cannot find sub agent on super_agent_event.sub_agent_became_unhealthy event");
                                        continue;
                                    };

                                    let _ = self.super_agent_publisher
                                    .publish(SuperAgentEvent::SubAgentBecameUnhealthy(agent_id,sub_agent.agent_type(), unhealthy, start_time))
                                    .inspect_err(|err| error!(error_msg = %err,"cannot publish super_agent_event.sub_agent_became_unhealthy"));
                                },
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // apply a remote config to the running sub agents
    pub(super) fn apply_remote_super_agent_config(
        &self,
        remote_config: &RemoteConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        // Fail if the remote config has already identified as failed.
        if let Some(err) = remote_config.hash.error_message() {
            // TODO seems like this error should be sent by the remote config itself
            return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
        }

        let remote_config_value = remote_config.get_unique()?;

        let old_super_agent_dynamic_config = self.sa_dynamic_config_store.load()?;

        let super_agent_dynamic_config = if remote_config_value.is_empty() {
            // Use the local configuration if the content of the remote config is empty.
            // Do not confuse with an empty list of 'agents', which is a valid remote configuration.
            self.sa_dynamic_config_store.delete()?;
            self.sa_dynamic_config_store.load()?
        } else {
            SuperAgentDynamicConfig::try_from(remote_config_value)?
        };

        // TODO the case when multiple agents are updated but one fails has multiple issues:
        // - old agents keeps running
        // - some agents could be created and some not independently if they have correct configs since fails on first error
        // - storers isn't updated (event for an agent that has been applied and running )

        // apply new configuration
        super_agent_dynamic_config
            .agents
            .iter()
            .try_for_each(|(agent_id, agent_config)| {
                // recreates an existent sub agent if the configuration has changed
                match old_super_agent_dynamic_config.agents.get(agent_id) {
                    Some(old_sub_agent_config) => {
                        if old_sub_agent_config == agent_config {
                            return Ok(());
                        }

                        info!("Recreating SubAgent {}", agent_id);
                        self.recreate_sub_agent(
                            agent_id.clone(),
                            agent_config,
                            running_sub_agents,
                            sub_agent_publisher.clone(),
                        )
                    }
                    None => {
                        info!("Creating SubAgent {}", agent_id);
                        self.create_sub_agent(
                            agent_id.clone(),
                            agent_config,
                            running_sub_agents,
                            sub_agent_publisher.clone(),
                        )
                    }
                }
            })?;

        // remove sub agents not used anymore
        old_super_agent_dynamic_config.agents.iter().try_for_each(
            |(agent_id, _agent_config)| {
                if !super_agent_dynamic_config.agents.contains_key(agent_id) {
                    info!("Stopping SubAgent {}", agent_id);

                    let _ = self
                        .super_agent_publisher
                        .publish(SuperAgentEvent::SubAgentRemoved(agent_id.clone()))
                        .inspect_err(|err| {
                            error!(
                                error_msg = %err,
                                "cannot publish super_agent_event.sub_agent_removed"
                            )
                        });

                    return running_sub_agents.stop_remove(agent_id);
                }
                Ok(())
            },
        )?;

        if !remote_config_value.is_empty() {
            self.sa_dynamic_config_store
                .store(&super_agent_dynamic_config)?;
        }

        Ok(self
            .remote_config_hash_repository
            .save(&self.agent_id, &remote_config.hash)?)
    }

    pub(crate) fn report_healthy(&self, healthy: Healthy) -> Result<(), AgentError> {
        self.report_health(healthy.clone().into())?;
        Ok(self
            .super_agent_publisher
            .publish(SuperAgentEvent::SuperAgentBecameHealthy(healthy))?)
    }

    pub(crate) fn report_unhealthy(&self, unhealthy: Unhealthy) -> Result<(), AgentError> {
        self.report_health(unhealthy.clone().into())?;
        Ok(self
            .super_agent_publisher
            .publish(SuperAgentEvent::SuperAgentBecameUnhealthy(unhealthy))?)
    }

    fn report_health(&self, health: Health) -> Result<(), AgentError> {
        if let Some(handle) = &self.opamp_client {
            debug!(
                is_healthy = health.is_healthy().to_string(),
                "Sending super-agent health"
            );

            let health = ComponentHealth {
                healthy: health.is_healthy(),
                start_time_unix_nano: get_sys_time_nano()?,
                status: health.status().to_string(),
                last_error: health.last_error().unwrap_or_default().to_string(),
                ..Default::default()
            };

            handle.set_health(health)?;
        }
        Ok(())
    }
}

pub fn super_agent_fqn() -> AgentTypeFQN {
    AgentTypeFQN::try_from(
        format!(
            "{}/{}:{}",
            SUPER_AGENT_NAMESPACE(),
            SUPER_AGENT_TYPE(),
            SUPER_AGENT_VERSION()
        )
        .as_str(),
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::{ApplicationEvent, OpAMPEvent, SubAgentEvent, SuperAgentEvent};
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::hash_repository::HashRepository;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::sub_agent::{test::MockSubAgentBuilderMock, SubAgentBuilder};
    use crate::super_agent::config::{
        AgentID, AgentTypeFQN, SubAgentConfig, SuperAgentDynamicConfig,
    };
    use crate::super_agent::config_storer::loader_storer::tests::MockSuperAgentDynamicConfigStore;
    use crate::super_agent::config_storer::loader_storer::{
        SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
        SuperAgentDynamicConfigStorer,
    };
    use crate::super_agent::SuperAgent;
    use mockall::predicate;

    use crate::opamp::LastErrorMessage;
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::test::{MockNotStartedSubAgent, MockStartedSubAgent};
    use opamp_client::StartedClient;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::{Duration, SystemTime};

    use super::SuperAgentCallbacks;

    ////////////////////////////////////////////////////////////////////////////////////
    // Custom Agent constructor for tests
    ////////////////////////////////////////////////////////////////////////////////////
    impl<S, O, HR, SL> SuperAgent<S, O, HR, SL>
    where
        O: StartedClient<SuperAgentCallbacks>,
        HR: HashRepository,
        S: SubAgentBuilder,
        SL: SuperAgentDynamicConfigStorer
            + SuperAgentDynamicConfigLoader
            + SuperAgentDynamicConfigDeleter,
    {
        pub fn new_custom(
            opamp_client: Option<O>,
            remote_config_hash_repository: Arc<HR>,
            sub_agent_builder: S,
            sub_agents_config_store: SL,
            super_agent_publisher: EventPublisher<SuperAgentEvent>,
        ) -> Self {
            SuperAgent {
                opamp_client,
                remote_config_hash_repository,
                sub_agent_builder,
                agent_id: AgentID::new_super_agent_id(),
                sa_dynamic_config_store: Arc::new(sub_agents_config_store),
                super_agent_publisher,
            }
        }
    }

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_healthy();
        started_client.should_stop(1);

        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(HashMap::new().into()));

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(Some(hash))
        });

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        // no agents in the supervisor group
        let agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            MockSubAgentBuilderMock::new(),
            sub_agents_config_store,
            super_agent_publisher,
        );

        let (application_event_publisher, application_event_consumer) = pub_sub();

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent
            .run(application_event_consumer, pub_sub().1.into())
            .is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        let sub_agents_config = sub_agents_default_config();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_healthy();
        started_client.should_stop(1);

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(Some(hash))
        });

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);

        sub_agents_config_store
            .expect_load()
            .returning(move || Ok(sub_agents_config.clone()));

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        let agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
            super_agent_publisher,
        );

        let (application_event_publisher, application_event_consumer) = pub_sub();

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent
            .run(application_event_consumer, pub_sub().1.into())
            .is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));
        started_client.should_stop(1);

        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
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
                Ok(Some(hash))
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

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();

        let running_agent = spawn({
            move || {
                let (super_agent_publisher, _super_agent_consumer) = pub_sub();
                // two agents in the supervisor group
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );
                agent.run(application_event_consumer, opamp_consumer.into())
            }
        });

        let remote_config = RemoteConfig::new(
            AgentID::new_super_agent_id(),
            Hash::new("a-hash".to_string()),
            Some(ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.1"
"#
                .to_string(),
            )]))),
        );

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(500));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(running_agent.join().is_ok())
    }

    #[test]
    fn receive_opamp_connected() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let running_agent = spawn({
            move || {
                // two agents in the supervisor group
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );
                agent.process_events(
                    application_event_consumer,
                    Some(opamp_consumer),
                    sub_agent_publisher,
                    sub_agent_consumer,
                    sub_agents,
                )
            }
        });

        opamp_publisher.publish(OpAMPEvent::Connected).unwrap();

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::OpAMPConnected;
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(running_agent.join().is_ok());
    }

    #[test]
    fn receive_opamp_connect_failed() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let running_agent = spawn({
            move || {
                // two agents in the supervisor group
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );
                agent.process_events(
                    application_event_consumer,
                    Some(opamp_consumer),
                    sub_agent_publisher,
                    sub_agent_consumer,
                    sub_agents,
                )
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::ConnectFailed(
                Some(500),
                "Internal error".to_string(),
            ))
            .unwrap();

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::OpAMPConnectFailed(Some(500), "Internal error".to_string());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(running_agent.join().is_ok())
    }

    #[test]
    fn create_stop_sub_agents_from_remote_config() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config().agents;

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three times (2 + 1 + 1)
        sub_agent_builder.should_build(3);

        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
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
                        agent_type: AgentTypeFQN::try_from(
                            "newrelic/io.opentelemetry.collector:0.0.1",
                        )
                        .unwrap(),
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

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None::<MockStartedOpAMPClientMock<SuperAgentCallbacks>>,
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
            super_agent_publisher,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();

        let sub_agents = super_agent.load_sub_agents(&sub_agents_config, opamp_publisher.clone());

        let mut running_sub_agents = sub_agents.unwrap().run();

        // just one agent, it should remove the infra-agent
        let remote_config = RemoteConfig::new(
            AgentID::new_super_agent_id(),
            Hash::new("a-hash".to_string()),
            Some(ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#
                .to_string(),
            )]))),
        );

        assert_eq!(running_sub_agents.len(), 2);

        super_agent
            .apply_remote_super_agent_config(
                &remote_config,
                &mut running_sub_agents,
                opamp_publisher.clone(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let remote_config = RemoteConfig::new(
            AgentID::new_super_agent_id(),
            Hash::new("b-hash".to_string()),
            Some(ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.0.1
"#
                .to_string(),
            )]))),
        );

        super_agent
            .apply_remote_super_agent_config(
                &remote_config,
                &mut running_sub_agents,
                opamp_publisher.clone(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        assert!(running_sub_agents.stop().is_ok())
    }

    #[test]
    fn test_sub_agent_config_updated_should_recreate_sub_agent() {
        let hash_repository_mock = Arc::new(MockHashRepositoryMock::new());
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

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

        let sub_agents_config = SuperAgentDynamicConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from("namespace/fqn_rdot:0.0.1").unwrap(),
                },
            ),
            (
                sub_agent_id.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from("namespace/fqn_infra_agent:0.0.1").unwrap(),
                },
            ),
            (
                AgentID::new("fluent-bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from("namespace/fqn_fluent_bit:0.0.1").unwrap(),
                },
            ),
        ]));

        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get_mut(&sub_agent_id).unwrap().should_stop();
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
                agent_type: AgentTypeFQN::try_from("namespace/fqn_infra_agent:0.0.1").unwrap(),
            },
            not_started_sub_agent,
        );
        // And all the Sub Agents should stop on Stopping the Super Agent
        sub_agents
            .get_mut(&AgentID::new("nrdot").unwrap())
            .unwrap()
            .should_stop();
        sub_agents
            .get_mut(&AgentID::new("fluent-bit").unwrap())
            .unwrap()
            .should_stop();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (_super_agent_opamp_publisher, super_agent_opamp_consumer) = pub_sub();

        //OpAMP client should report healthy
        let mut opamp_client_mock = MockStartedOpAMPClientMock::new();
        opamp_client_mock.should_set_healthy();

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        // Create the Super Agent and run Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(opamp_client_mock),
            hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            super_agent_publisher,
        );

        let sub_agent_publisher_clone = sub_agent_publisher.clone();
        let application_event_publisher_clone = application_event_publisher.clone();
        spawn(move || {
            sleep(Duration::from_millis(20));

            sub_agent_publisher_clone
                .publish(SubAgentEvent::ConfigUpdated(
                    AgentID::new("infra-agent").unwrap(),
                ))
                .unwrap();

            application_event_publisher_clone
                .publish(ApplicationEvent::StopRequested)
                .unwrap();
        });

        super_agent
            .process_events(
                application_event_consumer,
                super_agent_opamp_consumer.into(),
                sub_agent_publisher,
                sub_agent_consumer,
                sub_agents,
            )
            .unwrap();
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Super Agent Events tests
    ////////////////////////////////////////////////////////////////////////////////////

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Super Agent Healthy event
    #[test]
    fn test_config_updated_should_publish_super_agent_healthy() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        // load local config
        let sub_agents_config = SuperAgentDynamicConfig::from(HashMap::default());
        sub_agents_config_store.should_load(&sub_agents_config);

        // store remote config
        sub_agents_config_store.should_store(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let remote_config = RemoteConfig::new(
            AgentID::new_super_agent_id(),
            remote_config_hash.clone(),
            Some(ConfigMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        // persist remote config hash as applying
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config_hash);

        // store super agent remote config hash
        remote_config_hash.apply();
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config_hash);

        // the running sub agent that will be stopped
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(
            AgentID::new("infra-agent").unwrap(),
            sub_agent,
        )]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(10));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an OpAMP Invalid Config should publish Unhealthy Event
    #[test]
    fn test_invalid_config_should_publish_super_agent_unhealthy() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        // set healthy on start processing events
        started_client.should_set_healthy();
        // set unhealthy on invalid config
        started_client.should_set_unhealthy();
        // applying and failed
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        remote_config_hash.fail(String::from("some error message"));

        let remote_config =
            RemoteConfig::new(AgentID::new_super_agent_id(), remote_config_hash, None);

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();

        sleep(Duration::from_millis(10));

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SuperAgentBecameUnhealthy(Unhealthy::new(String::default(),  String::from(
            "Error applying Super Agent remote config: remote config error: `config hash: `a-hash` config error: `some error message``",
        )));
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an StopRequest event should publish SuperAgentStopped
    #[test]
    fn test_stop_request_should_publish_super_agent_stopped() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        // set healthy on start processing events
        started_client.should_set_healthy();

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        sleep(Duration::from_millis(10));

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SuperAgentStopped;
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Publish SubAgentBecameHealthy
    #[test]
    fn test_sub_agent_became_healthy_should_published() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        // set healthy on start processing events
        started_client.should_set_healthy();

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        // the running sub agent that will be stopped
        let agent_id = AgentID::new("infra-agent").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-fqn:0.0.1").unwrap();
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();
        sub_agent.should_agent_type(agent_type.clone());

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let sub_agent_publisher_clone = sub_agent_publisher.clone();
        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher_clone,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        sleep(Duration::from_millis(10));

        let start_time = SystemTime::now();

        sub_agent_publisher
            .publish(SubAgentEvent::SubAgentBecameHealthy(
                agent_id.clone(),
                Healthy::default(),
                start_time,
            ))
            .unwrap();

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SubAgentBecameHealthy(
            agent_id,
            agent_type,
            Healthy::default(),
            start_time,
        );
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Publish SubAgentBecameUnhealthy
    #[test]
    fn test_sub_agent_became_unhealthy_should_published() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        // set healthy on start processing events
        started_client.should_set_healthy();

        let sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        // the running sub agent that will be stopped
        let agent_id = AgentID::new("infra-agent").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-fqn:0.0.1").unwrap();
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();
        sub_agent.should_agent_type(agent_type.clone());

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let sub_agent_publisher_clone = sub_agent_publisher.clone();
        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher_clone,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        sleep(Duration::from_millis(10));

        let last_error_message = LastErrorMessage::from("some last error message");
        sub_agent_publisher
            .publish(SubAgentEvent::SubAgentBecameUnhealthy(
                agent_id.clone(),
                Unhealthy::new(String::default(), last_error_message.clone()),
                SystemTime::UNIX_EPOCH,
            ))
            .unwrap();

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SubAgentBecameUnhealthy(
            agent_id,
            agent_type,
            Unhealthy::new(String::default(), last_error_message.clone()),
            SystemTime::UNIX_EPOCH,
        );
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Super Agent Healthy event
    // And it should publish SubAgentRemoved
    #[test]
    fn test_removing_a_sub_agent_should_publish_sub_agent_removed() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();

        let agent_id = AgentID::new("infra-agent").unwrap();

        // load local config
        let sub_agents_config = SuperAgentDynamicConfig::from(HashMap::from([(
            agent_id.clone(),
            SubAgentConfig {
                agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            },
        )]));
        sub_agents_config_store.should_load(&sub_agents_config);

        // store remote config
        let sub_agents_config = SuperAgentDynamicConfig::from(HashMap::default());
        sub_agents_config_store.should_store(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let remote_config = RemoteConfig::new(
            AgentID::new_super_agent_id(),
            remote_config_hash.clone(),
            Some(ConfigMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        // persist remote config hash as applying
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config_hash);

        // store super agent remote config hash
        remote_config_hash.apply();
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config_hash);

        // the running sub agent that will be stopped
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (super_agent_publisher, super_agent_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    sub_agents_config_store,
                    super_agent_publisher,
                );

                agent
                    .process_events(
                        application_event_consumer,
                        Some(opamp_consumer),
                        sub_agent_publisher,
                        sub_agent_consumer,
                        sub_agents,
                    )
                    .unwrap();
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(10));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with SuperAgentHealthy
        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SubAgentRemoved(agent_id);
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = SuperAgentEvent::SuperAgentBecameHealthy(Healthy::default());
        let ev = super_agent_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////

    fn sub_agents_default_config() -> SuperAgentDynamicConfig {
        HashMap::from([
            (
                AgentID::new("infra-agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                    )
                    .unwrap(),
                },
            ),
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from("newrelic/io.opentelemetry.collector:0.0.1")
                        .unwrap(),
                },
            ),
        ])
        .into()
    }
}
