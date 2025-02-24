use super::config::{AgentControlDynamicConfig, AgentID, SubAgentConfig, SubAgentsMap};
use super::config_storer::loader_storer::{
    AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
    AgentControlDynamicConfigStorer,
};
use crate::agent_control::config_validator::DynamicConfigValidator;
use crate::agent_control::error::AgentError;
use crate::event::{
    channel::{EventConsumer, EventPublisher},
    AgentControlEvent, ApplicationEvent, OpAMPEvent, SubAgentEvent,
};
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::{
    hash_repository::HashRepository,
    remote_config::hash::Hash,
    remote_config::{RemoteConfig, RemoteConfigError},
};
use crate::sub_agent::health::health_checker::{Health, Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::{
    collection::{NotStartedSubAgents, StartedSubAgents},
    error::SubAgentBuilderError,
    NotStartedSubAgent, SubAgentBuilder,
};
use crate::values::yaml_config::YAMLConfig;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::StartedClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

pub struct AgentControl<S, O, HR, SL, DV>
where
    O: StartedClient,
    HR: HashRepository,
    SL: AgentControlDynamicConfigStorer
        + AgentControlDynamicConfigLoader
        + AgentControlDynamicConfigDeleter,
    S: SubAgentBuilder,
    DV: DynamicConfigValidator,
{
    pub(super) opamp_client: Option<O>,
    sub_agent_builder: S,
    remote_config_hash_repository: Arc<HR>,
    agent_id: AgentID,
    start_time: SystemTime,
    pub(super) sa_dynamic_config_store: Arc<SL>,
    pub(super) agent_control_publisher: EventPublisher<AgentControlEvent>,
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    dynamic_config_validator: DV,
}

impl<S, O, HR, SL, DV> AgentControl<S, O, HR, SL, DV>
where
    O: StartedClient,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigStorer
        + AgentControlDynamicConfigLoader
        + AgentControlDynamicConfigDeleter,
    DV: DynamicConfigValidator,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opamp_client: Option<O>,
        remote_config_hash_repository: Arc<HR>,
        sub_agent_builder: S,
        sub_agents_config_store: Arc<SL>,
        agent_control_publisher: EventPublisher<AgentControlEvent>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        application_event_consumer: EventConsumer<ApplicationEvent>,
        agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        dynamic_config_validator: DV,
    ) -> Self {
        Self {
            opamp_client,
            remote_config_hash_repository,
            sub_agent_builder,
            // unwrap as we control content of the AGENT_CONTROL_ID constant
            agent_id: AgentID::new_agent_control_id(),
            start_time: SystemTime::now(),
            sa_dynamic_config_store: sub_agents_config_store,
            agent_control_publisher,
            sub_agent_publisher,
            application_event_consumer,
            agent_control_opamp_consumer,
            dynamic_config_validator,
        }
    }

    pub fn run(self) -> Result<(), AgentError> {
        debug!("Creating agent's communication channels");
        if let Some(opamp_client) = &self.opamp_client {
            match self.remote_config_hash_repository.get(&self.agent_id) {
                Err(e) => {
                    warn!("Failed getting remote config hash from the store: {}", e);
                }
                Ok(Some(mut hash)) => {
                    if !hash.is_applied() {
                        OpampRemoteConfigStatus::Applied.report(opamp_client, &hash)?;
                        self.set_config_hash_as_applied(&mut hash)?;
                    }
                }
                Ok(None) => {
                    warn!("OpAMP enabled but no previous remote configuration found");
                }
            }
            opamp_client.update_effective_config()?
        }

        info!("Starting the agents supervisor runtime");
        let sub_agents_config = self.sa_dynamic_config_store.load()?.agents;

        let not_started_sub_agents = self.build_sub_agents(&sub_agents_config)?;

        // RETURNS CHANNEL_TO_STOP (WHEN GRACEFUL OR CONFIG REMOVAL STOP) AND JOIN_HANDLE
        // Run all the Sub Agents
        let running_sub_agents = not_started_sub_agents.run();

        info!("Agents supervisor runtime successfully started");

        self.process_events(running_sub_agents);

        if let Some(opamp_client) = self.opamp_client {
            info!("Stopping the OpAMP Client");
            opamp_client.stop()?;
        }

        info!("AgentControl finished");
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
    fn build_sub_agents(
        &self,
        sub_agents: &SubAgentsMap,
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
                        self.sub_agent_publisher.clone(),
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
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_remove(&agent_id)?;

        self.create_sub_agent(agent_id, sub_agent_config, running_sub_agents)
    }

    // runs and adds into the sub_agents collection the given agent
    fn create_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        running_sub_agents.insert(
            agent_id.clone(),
            self.sub_agent_builder
                .build(agent_id, sub_agent_config, self.sub_agent_publisher.clone())?
                .run(),
        );

        Ok(())
    }

    // process_events listens for events from the Agent Control and the Sub Agents
    // This is the main thread loop, executed after initialization of all Agent Control components.
    fn process_events(
        &self,
        mut sub_agents: StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) {
        let _ = self
            .report_healthy(Healthy::new(String::default()))
            .inspect_err(
                |err| error!(error_msg = %err,"Error reporting health on Agent Control start"),
            );

        debug!("Listening for events from agents");
        let never_receive = EventConsumer::from(never());
        let opamp_receiver = self
            .agent_control_opamp_consumer
            .as_ref()
            .unwrap_or(&never_receive);
        loop {
            select! {
                recv(&opamp_receiver.as_ref()) -> opamp_event_res => {
                    match opamp_event_res {
                        Err(_) => {
                            debug!("channel closed");
                        },
                        Ok(opamp_event) => {
                            match opamp_event {
                                OpAMPEvent::RemoteConfigReceived(remote_config) => {
                                    let _ = self.remote_config(remote_config, &mut sub_agents)
                                    .inspect_err(|err| error!(error_msg = %err,"Error processing valid remote config"));
                                }
                                OpAMPEvent::Connected => {
                                    let _ = self.agent_control_publisher
                                    .publish(AgentControlEvent::OpAMPConnected)
                                    .inspect_err(|err| error!(error_msg = %err,"cannot publish agent_control_event::agent_control_opamp_connected"));
                                }
                                OpAMPEvent::ConnectFailed(error_code, error_message) => {
                                    let _ = self.agent_control_publisher
                                    .publish(AgentControlEvent::OpAMPConnectFailed(error_code, error_message))
                                    .inspect_err(|err| error!(error_msg = %err,"cannot publish agent_control_event::agent_control_opamp_connect_failed"));
                                }
                            }
                        }
                    }
                },
                recv(self.application_event_consumer.as_ref()) -> _agent_control_event => {
                    debug!("stopping Agent Control event processor");

                    let _ = self.agent_control_publisher
                    .publish(AgentControlEvent::AgentControlStopped)
                    .inspect_err(|err| error!(error_msg = %err,"cannot publish agent_control_event::agent_control_stopped"));

                    break sub_agents.stop();
                },
            }
        }
    }

    // apply a agent control remote config
    pub(super) fn apply_remote_agent_control_config(
        &self,
        remote_config: &RemoteConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        // Fail if the remote config has already identified as failed.
        if let Some(err) = remote_config.hash.error_message() {
            // TODO seems like this error should be sent by the remote config itself
            return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
        }

        let remote_config_value = remote_config.get_unique()?;

        let old_agent_control_dynamic_config = self.sa_dynamic_config_store.load()?;

        let agent_control_dynamic_config = if remote_config_value.is_empty() {
            // Use the local configuration if the content of the remote config is empty.
            // Do not confuse with an empty list of 'agents', which is a valid remote configuration.
            self.sa_dynamic_config_store.delete()?;
            self.sa_dynamic_config_store.load()?
        } else {
            AgentControlDynamicConfig::try_from(remote_config_value)?
        };

        debug!(
            "Performing validation for Agent Control remote configuration: {}",
            remote_config_value
        );
        self.dynamic_config_validator
            .validate(&agent_control_dynamic_config)?;

        self.apply_remote_agent_control_config_agents(
            old_agent_control_dynamic_config,
            agent_control_dynamic_config,
            running_sub_agents,
        )?;

        if !remote_config_value.is_empty() {
            self.sa_dynamic_config_store
                .store(&YAMLConfig::try_from(remote_config_value.to_string())?)?;
        }

        Ok(self
            .remote_config_hash_repository
            .save(&self.agent_id, &remote_config.hash)?)
    }

    // apply a remote config to the running sub agents
    pub(super) fn apply_remote_agent_control_config_agents(
        &self,
        old_agent_control_dynamic_config: AgentControlDynamicConfig,
        agent_control_dynamic_config: AgentControlDynamicConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        // TODO the case when multiple agents are updated but one fails has multiple issues:
        // - old agents keeps running
        // - some agents could be created and some not independently if they have correct configs since fails on first error
        // - storers isn't updated (event for an agent that has been applied and running )

        // apply new configuration
        agent_control_dynamic_config
            .agents
            .iter()
            .try_for_each(|(agent_id, agent_config)| {
                // recreates an existent sub agent if the configuration has changed
                match old_agent_control_dynamic_config.agents.get(agent_id) {
                    Some(old_sub_agent_config) => {
                        if old_sub_agent_config == agent_config {
                            return Ok(());
                        }

                        info!("Recreating SubAgent {}", agent_id);
                        self.recreate_sub_agent(agent_id.clone(), agent_config, running_sub_agents)
                    }
                    None => {
                        info!("Creating SubAgent {}", agent_id);
                        self.create_sub_agent(agent_id.clone(), agent_config, running_sub_agents)
                    }
                }
            })?;

        // remove sub agents not used anymore
        old_agent_control_dynamic_config
            .agents
            .iter()
            .try_for_each(|(agent_id, _agent_config)| {
                if !agent_control_dynamic_config.agents.contains_key(agent_id) {
                    info!("Stopping SubAgent {}", agent_id);

                    let _ = self
                        .agent_control_publisher
                        .publish(AgentControlEvent::SubAgentRemoved(agent_id.clone()))
                        .inspect_err(|err| {
                            error!(
                                error_msg = %err,
                                "cannot publish agent_control_event.sub_agent_removed"
                            )
                        });

                    return running_sub_agents.stop_remove(agent_id);
                }
                Ok(())
            })?;

        Ok(())
    }

    pub(crate) fn report_healthy(&self, healthy: Healthy) -> Result<(), AgentError> {
        self.report_health(healthy.clone().into())?;
        Ok(self
            .agent_control_publisher
            .publish(AgentControlEvent::AgentControlBecameHealthy(healthy))?)
    }

    pub(crate) fn report_unhealthy(&self, unhealthy: Unhealthy) -> Result<(), AgentError> {
        self.report_health(unhealthy.clone().into())?;
        Ok(self
            .agent_control_publisher
            .publish(AgentControlEvent::AgentControlBecameUnhealthy(unhealthy))?)
    }

    fn report_health(&self, health: Health) -> Result<(), AgentError> {
        if let Some(handle) = &self.opamp_client {
            debug!(
                is_healthy = health.is_healthy().to_string(),
                "Sending agent-control health"
            );

            handle.set_health(HealthWithStartTime::new(health, self.start_time).into())?;
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::agent_control::config::{
        AgentControlDynamicConfig, AgentID, AgentTypeFQN, SubAgentConfig,
    };
    use crate::agent_control::config_storer::loader_storer::tests::MockAgentControlDynamicConfigStore;
    use crate::agent_control::config_validator::tests::MockDynamicConfigValidatorMock;
    use crate::agent_control::config_validator::DynamicConfigValidatorError;
    use crate::agent_control::AgentControl;
    use crate::agent_type::agent_type_registry::AgentRepositoryError;
    use crate::event::channel::pub_sub;
    use crate::event::{AgentControlEvent, ApplicationEvent, OpAMPEvent};
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::sub_agent::tests::MockStartedSubAgent;
    use crate::sub_agent::tests::MockSubAgentBuilderMock;
    use mockall::predicate;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();
        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        started_client.should_set_healthy();
        started_client.should_update_effective_config(1);
        started_client.should_stop(1);

        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(HashMap::new().into()));

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(Some(hash))
        });

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        // no agents in the supervisor group
        let agent = AgentControl::new(
            Some(started_client),
            Arc::new(hash_repository_mock),
            MockSubAgentBuilderMock::new(),
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            application_event_consumer,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent.run().is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        let sub_agents_config = sub_agents_default_config();

        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_healthy();
        started_client.should_update_effective_config(1);
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

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let agent = AgentControl::new(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            application_event_consumer,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent.run().is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));
        started_client.should_update_effective_config(2);
        started_client.should_stop(1);

        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
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
            .with(predicate::eq(AgentID::new_agent_control_id()))
            .times(1)
            .returning(|_| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.apply();
                Ok(Some(hash))
            });

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_agent_control_id()),
                predicate::eq(Hash::new("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_agent_control_id()),
                predicate::eq(Hash::applied("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let running_agent = spawn({
            move || {
                // two agents in the supervisor group
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );
                agent.run()
            }
        });

        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.1"
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

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        let sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let sub_agents = StartedSubAgents::from(HashMap::default());

        let running_agent = spawn({
            move || {
                // two agents in the supervisor group
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );
                agent.process_events(sub_agents)
            }
        });

        opamp_publisher.publish(OpAMPEvent::Connected).unwrap();

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::OpAMPConnected;
        let ev = agent_control_consumer.as_ref().recv().unwrap();
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

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);

        let sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let running_agent = spawn({
            move || {
                // two agents in the supervisor group
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );
                agent.process_events(sub_agents)
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::ConnectFailed(
                Some(500),
                "Internal error".to_string(),
            ))
            .unwrap();

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected =
            AgentControlEvent::OpAMPConnectFailed(Some(500), "Internal error".to_string());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
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

        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        dynamic_config_validator
            .expect_validate()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
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
            &AgentID::new_agent_control_id(),
            &Hash::new("a-hash".to_string()),
        );
        hash_repository_mock.should_save_hash(
            &AgentID::new_agent_control_id(),
            &Hash::new("b-hash".to_string()),
        );
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            None::<MockStartedOpAMPClientMock>,
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        let sub_agents = agent_control.build_sub_agents(&sub_agents_config);

        let mut running_sub_agents = sub_agents.unwrap().run();

        // just one agent, it should remove the infra-agent
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
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

        agent_control
            .apply_remote_agent_control_config(&remote_config, &mut running_sub_agents)
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("b-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure:0.0.1
"#
                .to_string(),
            )]))),
        );

        agent_control
            .apply_remote_agent_control_config(&remote_config, &mut running_sub_agents)
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        running_sub_agents.stop()
    }

    #[test]
    fn create_sub_agent_wrong_agent_type_from_remote_config() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config().agents;

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        sub_agent_builder.should_build(2);

        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| {
                Err(DynamicConfigValidatorError::from(
                    AgentRepositoryError::NotFound("not found".to_string()),
                ))
            });

        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        // all agents on first load
        sub_agents_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        let hash_repository_mock = MockHashRepositoryMock::new();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            None::<MockStartedOpAMPClientMock>,
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        let sub_agents = agent_control.build_sub_agents(&sub_agents_config);

        let mut running_sub_agents = sub_agents.unwrap().run();

        // just one agent, it should remove the infra-agent
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  nrdot:
    agent_type: newrelic/invented-agent-type:0.0.1

"#
                .to_string(),
            )]))),
        );

        assert_eq!(running_sub_agents.len(), 2);

        let apply_remote = agent_control
            .apply_remote_agent_control_config(&remote_config, &mut running_sub_agents);

        assert!(apply_remote.is_err());

        running_sub_agents.stop();
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Events tests
    ////////////////////////////////////////////////////////////////////////////////////

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Agent Control Healthy event
    #[test]
    fn test_config_updated_should_publish_agent_control_healthy() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        started_client.should_update_effective_config(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        // load local config
        let sub_agents_config = AgentControlDynamicConfig::from(HashMap::default());
        sub_agents_config_store.should_load(&sub_agents_config);

        // store remote config
        sub_agents_config_store.should_store(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            remote_config_hash.clone(),
            Some(ConfigurationMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        // persist remote config hash as applying
        hash_repository_mock
            .should_save_hash(&AgentID::new_agent_control_id(), &remote_config_hash);

        // store agent control remote config hash
        remote_config_hash.apply();
        hash_repository_mock
            .should_save_hash(&AgentID::new_agent_control_id(), &remote_config_hash);

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
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );

                agent.process_events(sub_agents);
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

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an OpAMP Invalid Config should publish Unhealthy Event
    #[test]
    fn test_invalid_config_should_publish_agent_control_unhealthy() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Agent Control OpAMP
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

        let sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        remote_config_hash.fail(String::from("some error message"));

        let remote_config =
            RemoteConfig::new(AgentID::new_agent_control_id(), remote_config_hash, None);

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );

                agent.process_events(sub_agents);
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

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::AgentControlBecameUnhealthy(Unhealthy::new(String::default(),  String::from(
            "Error applying Agent Control remote config: remote config error: `config hash: `a-hash` config error: `some error message``",
        )));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an StopRequest event should publish AgentControlStopped
    #[test]
    fn test_stop_request_should_publish_agent_control_stopped() {
        let hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        // set healthy on start processing events
        started_client.should_set_healthy();

        let sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );

                agent.process_events(sub_agents);
            }
        });

        sleep(Duration::from_millis(10));

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::AgentControlStopped;
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Agent Control Healthy event
    // And it should publish SubAgentRemoved
    #[test]
    fn test_removing_a_sub_agent_should_publish_sub_agent_removed() {
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(2);
        started_client.should_update_effective_config(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();

        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        let agent_id = AgentID::new("infra-agent").unwrap();

        // load local config
        let sub_agents_config = AgentControlDynamicConfig::from(HashMap::from([(
            agent_id.clone(),
            SubAgentConfig {
                agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            },
        )]));
        sub_agents_config_store.should_load(&sub_agents_config);

        // store remote config
        let sub_agents_config = AgentControlDynamicConfig::from(HashMap::default());
        sub_agents_config_store.should_store(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            remote_config_hash.clone(),
            Some(ConfigurationMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        // persist remote config hash as applying
        hash_repository_mock
            .should_save_hash(&AgentID::new_agent_control_id(), &remote_config_hash);

        // store agent control remote config hash
        remote_config_hash.apply();
        hash_repository_mock
            .should_save_hash(&AgentID::new_agent_control_id(), &remote_config_hash);

        // the running sub agent that will be stopped
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let agent = AgentControl::new(
                    Some(started_client),
                    Arc::new(hash_repository_mock),
                    sub_agent_builder,
                    Arc::new(sub_agents_config_store),
                    agent_control_publisher,
                    sub_agent_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                );

                agent.process_events(sub_agents);
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

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::SubAgentRemoved(agent_id);
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::AgentControlBecameHealthy(Healthy::default());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////

    fn sub_agents_default_config() -> AgentControlDynamicConfig {
        HashMap::from([
            (
                AgentID::new("infra-agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "newrelic/com.newrelic.infrastructure:0.0.1",
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
