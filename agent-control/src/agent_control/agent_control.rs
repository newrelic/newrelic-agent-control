use super::config::{
    AgentControlConfig, AgentControlDynamicConfig, SubAgentsMap, sub_agents_difference,
};
use super::config_repository::repository::AgentControlDynamicConfigRepository;
use super::resource_cleaner::ResourceCleaner;
use super::version_updater::VersionUpdater;
use crate::agent_control::config_validator::DynamicConfigValidator;
use crate::agent_control::error::AgentError;
use crate::agent_control::uptime_report::UptimeReporter;
use crate::event::{
    AgentControlEvent, ApplicationEvent, OpAMPEvent, SubAgentEvent,
    broadcaster::unbounded::UnboundedBroadcast, channel::EventConsumer,
};
use crate::health::health_checker::{Health, Healthy, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::remote_config::{
    OpampRemoteConfig, OpampRemoteConfigError, hash::ConfigState, report::OpampRemoteConfigStatus,
};
use crate::sub_agent::{
    NotStartedSubAgent, SubAgentBuilder, collection::StartedSubAgents, identity::AgentIdentity,
};
use crate::values::config::RemoteConfig as RemoteConfigValues;
use crate::values::yaml_config::YAMLConfig;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::StartedClient;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info, instrument, trace, warn};

pub struct AgentControl<S, O, SL, DV, RC, VU>
where
    O: StartedClient,
    SL: AgentControlDynamicConfigRepository,
    S: SubAgentBuilder,
    DV: DynamicConfigValidator,
    RC: ResourceCleaner,
    VU: VersionUpdater,
{
    pub(super) opamp_client: Option<O>,
    sub_agent_builder: S,
    start_time: SystemTime,
    pub(super) sa_dynamic_config_store: Arc<SL>,
    pub(super) agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    dynamic_config_validator: DV,
    resource_cleaner: RC,
    _version_updater: VU,
    initial_config: AgentControlConfig,
}

impl<S, O, SL, DV, RC, VU> AgentControl<S, O, SL, DV, RC, VU>
where
    O: StartedClient,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigRepository,
    DV: DynamicConfigValidator,
    RC: ResourceCleaner,
    VU: VersionUpdater,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opamp_client: Option<O>,
        sub_agent_builder: S,
        sa_dynamic_config_store: Arc<SL>,
        agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
        sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
        application_event_consumer: EventConsumer<ApplicationEvent>,
        agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        dynamic_config_validator: DV,
        resource_cleaner: RC,
        version_updater: VU,
        initial_config: AgentControlConfig,
    ) -> Self {
        Self {
            opamp_client,
            sub_agent_builder,
            // unwrap as we control content of the AGENT_CONTROL_ID constant
            start_time: SystemTime::now(),
            sa_dynamic_config_store,
            agent_control_publisher,
            sub_agent_publisher,
            application_event_consumer,
            agent_control_opamp_consumer,
            dynamic_config_validator,
            resource_cleaner,
            _version_updater: version_updater,
            initial_config,
        }
    }

    pub fn run(self) -> Result<(), AgentError> {
        debug!("Creating agent's communication channels");
        if let Some(opamp_client) = &self.opamp_client {
            match self.sa_dynamic_config_store.get_hash() {
                Err(e) => {
                    warn!("Failed getting remote config hash from the store: {}", e);
                }
                Ok(Some(hash)) => {
                    if !hash.is_applied() {
                        OpampRemoteConfigStatus::Applied.report(opamp_client, hash.get())?;
                        self.sa_dynamic_config_store
                            .update_hash_state(&ConfigState::Applied)?;
                    }
                }
                Ok(None) => {
                    warn!("OpAMP enabled but no previous remote configuration found");
                }
            }
            opamp_client.update_effective_config()?
        }

        info!("Starting the agents supervisor runtime");
        // This is a first-time run and we already read the config earlier, the `initial_config` contains
        // the result as read by the `AgentControlConfigLoader`.
        let sub_agents_config = &self.initial_config.dynamic.agents;

        let running_sub_agents = self.build_and_run_sub_agents(sub_agents_config)?;

        info!("Agents supervisor runtime successfully started");

        self.process_events(running_sub_agents);

        if let Some(opamp_client) = self.opamp_client {
            info!("Stopping the OpAMP Client");
            opamp_client.stop()?;
        }

        info!("AgentControl finished");
        Ok(())
    }

    // Recreates a Sub Agent by its agent_id meaning:
    //  * Remove and stop the existing running Sub Agent from the Running Sub Agents
    //  * Recreate the Final Agent using the Agent Type and the latest persisted config
    //  * Build a Stopped Sub Agent
    //  * Run the Sub Agent and add it to the Running Sub Agents
    #[instrument(skip_all)]
    pub(super) fn recreate_sub_agent(
        &self,
        agent_identity: &AgentIdentity,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_and_remove(&agent_identity.id)?;
        self.build_and_run_sub_agent(agent_identity, running_sub_agents)
    }

    // build_sub_agents returns a collection of started sub agents
    fn build_and_run_sub_agents(
        &self,
        sub_agents: &SubAgentsMap,
    ) -> Result<
        StartedSubAgents<<S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent>,
        AgentError,
    > {
        let mut running_sub_agents = StartedSubAgents::default();

        for (agent_id, agent_config) in sub_agents {
            let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));

            self.build_and_run_sub_agent(&agent_identity, &mut running_sub_agents)?;
        }
        Ok(running_sub_agents)
    }

    // runs and adds into the sub_agents collection the given agent
    fn build_and_run_sub_agent(
        &self,
        agent_identity: &AgentIdentity,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        running_sub_agents.insert(
            agent_identity.id.clone(),
            self.sub_agent_builder
                .build(agent_identity, self.sub_agent_publisher.clone())?
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
            .report_health(Healthy::new(String::default()).into())
            .inspect_err(
                |err| error!(error_msg = %err,"Error reporting health on Agent Control start"),
            );

        debug!("Listening for events from agents");
        let never_receive = EventConsumer::from(never());
        let opamp_receiver = self
            .agent_control_opamp_consumer
            .as_ref()
            .unwrap_or(&never_receive);

        let uptime_report_config = &self.initial_config.uptime_report;
        let uptime_reporter =
            UptimeReporter::from(uptime_report_config).with_start_time(self.start_time);
        // If a uptime report is configured, we trace it for the first time here
        if uptime_report_config.enabled() {
            let _ = uptime_reporter.report();
        }

        // Count the received remote configs during execution
        let mut remote_config_count = 0;
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
                                    // Report the reception of a remote config
                                    debug!("Received remote config.");
                                    remote_config_count += 1;
                                    trace!(monotonic_counter.remote_configs_received = remote_config_count);
                                    let _ = self.handle_remote_config(remote_config, &mut sub_agents)
                                        .inspect_err(|err| error!(error_msg = %err,"Error processing valid remote config"));
                                }
                                OpAMPEvent::Connected => self.agent_control_publisher.broadcast(AgentControlEvent::OpAMPConnected),
                                OpAMPEvent::ConnectFailed(error_code, error_message) => self.agent_control_publisher.broadcast(AgentControlEvent::OpAMPConnectFailed(error_code, error_message))
                            }
                        }
                    }
                },
                recv(self.application_event_consumer.as_ref()) -> _agent_control_event => {
                    debug!("stopping Agent Control event processor");
                    self.agent_control_publisher.broadcast(AgentControlEvent::AgentControlStopped);
                    break sub_agents.stop();
                },
                recv(uptime_reporter.receiver()) -> _tick => { let _ = uptime_reporter.report(); },
            }
        }
    }

    /// Agent Control on remote config
    /// Configuration will be reported as applying to OpAMP
    /// Valid configuration will be applied and reported as applied to OpAMP
    /// If the configuration is invalid, it will be reported as error to OpAMP
    pub(crate) fn handle_remote_config(
        &self,
        opamp_remote_config: OpampRemoteConfig,
        sub_agents: &mut StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let Some(opamp_client) = &self.opamp_client else {
            unreachable!("got remote config without OpAMP being enabled");
        };

        info!("Applying remote config");
        OpampRemoteConfigStatus::Applying.report(opamp_client, opamp_remote_config.hash.get())?;

        match self.apply_remote_agent_control_config(&opamp_remote_config, sub_agents) {
            Err(err) => {
                let error_message = format!("Error applying Agent Control remote config: {}", err);
                error!(error_message);
                OpampRemoteConfigStatus::Error(error_message.clone())
                    .report(opamp_client, opamp_remote_config.hash.get())?;
                Ok(self.report_health(Unhealthy::new(String::default(), error_message).into())?)
            }
            Ok(()) => {
                self.sa_dynamic_config_store
                    .update_hash_state(&ConfigState::Applied)?;
                OpampRemoteConfigStatus::Applied
                    .report(opamp_client, opamp_remote_config.hash.get())?;
                opamp_client.update_effective_config()?;
                Ok(self.report_health(Healthy::new(String::default()).into())?)
            }
        }
    }

    #[instrument(skip_all)]
    // apply an agent control remote config
    pub(super) fn apply_remote_agent_control_config(
        &self,
        opamp_remote_config: &OpampRemoteConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        // Fail if the remote config has already identified as failed.
        if let Some(err) = opamp_remote_config.hash.error_message() {
            // TODO seems like this error should be sent by the remote config itself
            return Err(
                OpampRemoteConfigError::InvalidConfig(opamp_remote_config.hash.get(), err).into(),
            );
        }

        let remote_config_value = opamp_remote_config.get_unique()?;

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
            let config = RemoteConfigValues::new(
                YAMLConfig::try_from(remote_config_value.to_string())?,
                opamp_remote_config.hash.clone(),
            );
            self.sa_dynamic_config_store.store(&config)?;
        }

        Ok(())
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
                let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));
                // recreates an existent sub agent if the configuration has changed
                match old_agent_control_dynamic_config.agents.get(agent_id) {
                    Some(old_sub_agent_config) => {
                        if old_sub_agent_config == agent_config {
                            return Ok(());
                        }

                        info!("Recreating SubAgent {}", agent_id);
                        self.recreate_sub_agent(&agent_identity, running_sub_agents)
                    }
                    None => {
                        info!("Creating SubAgent {}", agent_id);
                        self.build_and_run_sub_agent(&agent_identity, running_sub_agents)
                    }
                }
            })?;

        // remove sub agents not used anymore
        let mut sub_agents_to_remove = sub_agents_difference(
            &old_agent_control_dynamic_config.agents,
            &agent_control_dynamic_config.agents,
        );
        sub_agents_to_remove.try_for_each(
            |(agent_id, agent_config)| -> Result<(), AgentError> {
                self.agent_control_publisher
                    .broadcast(AgentControlEvent::SubAgentRemoved(agent_id.clone()));

                running_sub_agents.stop_and_remove(agent_id)?;
                self.resource_cleaner
                    .clean(agent_id, &agent_config.agent_type)?;
                Ok(())
            },
        )?;

        Ok(())
    }

    pub fn report_health(&self, health: Health) -> Result<(), AgentError> {
        let health = HealthWithStartTime::new(health, self.start_time);
        if let Some(handle) = &self.opamp_client {
            debug!(
                is_healthy = health.is_healthy().to_string(),
                "Sending agent-control health"
            );

            handle.set_health(health.clone().into())?;
        }
        self.agent_control_publisher
            .broadcast(AgentControlEvent::HealthUpdated(health));
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::agent_control::AgentControl;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::tests::sub_agents_default_config;
    use crate::agent_control::config::{
        AgentControlConfig, AgentControlDynamicConfig, SubAgentConfig,
    };
    use crate::agent_control::config_repository::repository::tests::MockAgentControlDynamicConfigStore;
    use crate::agent_control::config_validator::DynamicConfigValidatorError;
    use crate::agent_control::config_validator::tests::MockDynamicConfigValidator;
    use crate::agent_control::resource_cleaner::ResourceCleanerError;
    use crate::agent_control::resource_cleaner::no_op::NoOpResourceCleaner;
    use crate::agent_control::resource_cleaner::tests::MockResourceCleaner;
    use crate::agent_control::version_updater::NoOpUpdater;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::agent_type_registry::AgentRepositoryError;
    use crate::event::broadcaster::unbounded::UnboundedBroadcast;
    use crate::event::channel::{EventConsumer, pub_sub};
    use crate::event::{AgentControlEvent, ApplicationEvent, OpAMPEvent};
    use crate::health::health_checker::{Healthy, Unhealthy};
    use crate::health::with_start_time::HealthWithStartTime;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::{ConfigurationMap, OpampRemoteConfig};
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::tests::MockStartedSubAgent;
    use crate::sub_agent::tests::MockSubAgentBuilder;
    use crate::values::config::RemoteConfig;
    use mockall::{Sequence, predicate};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::{Duration, SystemTime};

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        let mut started_client = MockStartedOpAMPClient::new();
        let dynamic_config_validator = MockDynamicConfigValidator::new();
        started_client.should_set_healthy();
        started_client.should_update_effective_config(1);
        started_client.should_stop(1);

        sa_dynamic_config_store
            .expect_load()
            .returning(|| Ok(AgentControlDynamicConfig::default()));

        sa_dynamic_config_store
            .expect_get_hash()
            .times(1)
            .returning(|| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.update_state(&ConfigState::Applied);
                Ok(Some(hash))
            });

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        // no agents in the supervisor group
        let agent_control = AgentControl::new(
            Some(started_client),
            MockSubAgentBuilder::new(),
            Arc::new(sa_dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            application_event_consumer,
            Some(opamp_consumer),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut sub_agent_builder = MockSubAgentBuilder::new();
        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let ac_config = AgentControlConfig {
            dynamic: sub_agents_default_config(),
            ..Default::default()
        };

        let dynamic_config_validator = MockDynamicConfigValidator::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_healthy();
        started_client.should_update_effective_config(1);
        started_client.should_stop(1);

        sa_dynamic_config_store
            .expect_get_hash()
            .times(1)
            .returning(|| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.update_state(&ConfigState::Applied);
                Ok(Some(hash))
            });

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        let agent_control = AgentControl::new(
            Some(started_client),
            sub_agent_builder,
            Arc::new(sa_dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            application_event_consumer,
            Some(opamp_consumer),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            ac_config,
        );

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent_control.run().is_ok())
    }

    #[test]
    // This tests makes sure that after receiving an "OpAMPEvent::ConnectFailed"
    // the AC reports that it is connected to OpAMP and it is healthy
    fn receive_opamp_connected() {
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_health(1);

        let sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidator::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());

        let sub_agents = StartedSubAgents::from(HashMap::default());

        let event_processor = spawn({
            move || {
                // two agents in the supervisor group
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch start_time to allow comparison
                agent.process_events(sub_agents);
            }
        });

        opamp_publisher.publish(OpAMPEvent::Connected).unwrap();

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));

        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::OpAMPConnected;
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());
    }

    #[test]
    // This tests makes sure that after receiving an "OpAMPEvent::Connected"
    // the AC reports that it is NOT connected to OpAMP
    fn receive_opamp_connect_failed() {
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_health(1);

        let sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidator::new();

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());

        let sub_agents = StartedSubAgents::from(HashMap::default());

        let event_processor = spawn({
            move || {
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch time to allow comparison
                agent.process_events(sub_agents);
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::ConnectFailed(
                Some(500),
                "Internal error".to_string(),
            ))
            .unwrap();

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected =
            AgentControlEvent::OpAMPConnectFailed(Some(500), "Internal error".to_string());
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok())
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Remote Config Tests
    ////////////////////////////////////////////////////////////////////////////////////

    #[test]
    fn receive_opamp_remote_config() {
        let mut sub_agent_builder = MockSubAgentBuilder::new();

        let ac_config = AgentControlConfig {
            dynamic: sub_agents_default_config(),
            ..Default::default()
        };

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_health(2);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));
        started_client.should_update_effective_config(2);
        started_client.should_stop(1);

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        sa_dynamic_config_store
            .expect_load()
            .once()
            .returning(|| Ok(sub_agents_default_config()));
        // updated agent
        sa_dynamic_config_store
            .expect_store()
            .once()
            .returning(|_| Ok(()));

        sa_dynamic_config_store
            .expect_get_hash()
            .times(1)
            .returning(|| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.update_state(&ConfigState::Applied);
                Ok(Some(hash))
            });

        sa_dynamic_config_store
            .expect_update_hash_state()
            .with(predicate::eq(ConfigState::Applied))
            .times(1)
            .returning(|_| Ok(()));

        // it should build two subagents: nrdot + infra-agent
        sub_agent_builder.should_build(2);
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let (application_event_publisher, application_event_consumer) = pub_sub();

        let running_agent_control = spawn({
            move || {
                // two agents in the supervisor group
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    UnboundedBroadcast::default(),
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    ac_config,
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch time to allow comparison
                agent.run()
            }
        });

        let opamp_remote_config = OpampRemoteConfig::new(
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
            .publish(OpAMPEvent::RemoteConfigReceived(opamp_remote_config))
            .unwrap();
        sleep(Duration::from_millis(500));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(running_agent_control.join().is_ok())
    }

    #[test]
    fn create_stop_sub_agents_from_remote_config() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config().agents;

        let mut sub_agent_builder = MockSubAgentBuilder::new();
        // it should build three times (2 + 1 + 1)
        sub_agent_builder.should_build(3);

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(2)
            .returning(|_| Ok(()));

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        // all agents on first load
        sa_dynamic_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        sa_dynamic_config_store
            .expect_load()
            .once()
            .return_once(|| {
                Ok(AgentControlDynamicConfig {
                    agents: HashMap::from([(
                        AgentID::new("nrdot").unwrap(),
                        SubAgentConfig {
                            agent_type: AgentTypeID::try_from(
                                "newrelic/io.opentelemetry.collector:0.0.1",
                            )
                            .unwrap(),
                        },
                    )]),
                    chart_version: None,
                })
            });

        sa_dynamic_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        sa_dynamic_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        let (_opamp_publisher, opamp_consumer) = pub_sub();

        let mut resource_cleaner = MockResourceCleaner::new();
        let mut resource_cleaning_seq = Sequence::new();
        let mut sub_agents_to_clean = sub_agents_default_config().agents;
        let infra_agent_id = AgentID::new("infra-agent").unwrap();
        let infra_agent_type = sub_agents_to_clean
            .remove(&infra_agent_id)
            .unwrap()
            .agent_type;
        let nrdot_agent_id = AgentID::new("nrdot").unwrap();
        let nrdot_agent_type = sub_agents_to_clean
            .remove(&nrdot_agent_id)
            .unwrap()
            .agent_type;
        // This test first cleans up the infra-agent agent
        resource_cleaner
            .expect_clean()
            .once()
            .in_sequence(&mut resource_cleaning_seq)
            .with(
                predicate::eq(infra_agent_id),
                predicate::eq(infra_agent_type),
            )
            .returning(|_, _| Ok(()));
        // Then cleans up the nrdot agent
        resource_cleaner
            .expect_clean()
            .once()
            .in_sequence(&mut resource_cleaning_seq)
            .with(
                predicate::eq(nrdot_agent_id),
                predicate::eq(nrdot_agent_type),
            )
            .returning(|_, _| Ok(()));

        // Create the Agent Control and run Sub Agents
        let agent_control = AgentControl::new(
            None::<MockStartedOpAMPClient>,
            sub_agent_builder,
            Arc::new(sa_dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
            resource_cleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        let mut running_sub_agents = agent_control
            .build_and_run_sub_agents(&sub_agents_config)
            .unwrap();

        // just one agent, it should remove the infra-agent
        let opamp_remote_config = OpampRemoteConfig::new(
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
            .apply_remote_agent_control_config(&opamp_remote_config, &mut running_sub_agents)
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let opamp_remote_config = OpampRemoteConfig::new(
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
            .apply_remote_agent_control_config(&opamp_remote_config, &mut running_sub_agents)
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        running_sub_agents.stop()
    }

    #[test]
    fn agent_control_fails_if_resource_cleaning_fails() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config().agents;

        let mut sub_agent_builder = MockSubAgentBuilder::new();
        // it should build three times (2 + 1 + 1)
        sub_agent_builder.should_build(3);

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(2)
            .returning(|_| Ok(()));

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        // all agents on first load
        sa_dynamic_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        sa_dynamic_config_store
            .expect_load()
            .once()
            .return_once(|| {
                Ok(AgentControlDynamicConfig {
                    agents: HashMap::from([(
                        AgentID::new("nrdot").unwrap(),
                        SubAgentConfig {
                            agent_type: AgentTypeID::try_from(
                                "newrelic/io.opentelemetry.collector:0.0.1",
                            )
                            .unwrap(),
                        },
                    )]),
                    chart_version: None,
                })
            });

        sa_dynamic_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        let (_opamp_publisher, opamp_consumer) = pub_sub();

        let mut resource_cleaner = MockResourceCleaner::new();
        let mut resource_cleaning_seq = Sequence::new();
        let mut sub_agents_to_clean = sub_agents_default_config().agents;
        let infra_agent_id = AgentID::new("infra-agent").unwrap();
        let infra_agent_type = sub_agents_to_clean
            .remove(&infra_agent_id)
            .unwrap()
            .agent_type;
        let nrdot_agent_id = AgentID::new("nrdot").unwrap();
        let nrdot_agent_type = sub_agents_to_clean
            .remove(&nrdot_agent_id)
            .unwrap()
            .agent_type;
        // This test first cleans up the infra-agent agent
        resource_cleaner
            .expect_clean()
            .once()
            .in_sequence(&mut resource_cleaning_seq)
            .with(
                predicate::eq(infra_agent_id),
                predicate::eq(infra_agent_type),
            )
            .returning(|_, _| Ok(()));
        // Then cleans up the nrdot agent
        resource_cleaner
            .expect_clean()
            .once()
            .in_sequence(&mut resource_cleaning_seq)
            .with(
                predicate::eq(nrdot_agent_id),
                predicate::eq(nrdot_agent_type),
            )
            .returning(|_, _| {
                Err(ResourceCleanerError::new(
                    "something failed when cleaning up resources!",
                ))
            });

        // Create the Agent Control and run Sub Agents
        let agent_control = AgentControl::new(
            None::<MockStartedOpAMPClient>,
            sub_agent_builder,
            Arc::new(sa_dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
            resource_cleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        let mut running_sub_agents = agent_control
            .build_and_run_sub_agents(&sub_agents_config)
            .unwrap();

        // just one agent, it should remove the infra-agent
        let opamp_remote_config = OpampRemoteConfig::new(
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
            .apply_remote_agent_control_config(&opamp_remote_config, &mut running_sub_agents)
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let opamp_remote_config = OpampRemoteConfig::new(
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

        assert!(
            agent_control
                .apply_remote_agent_control_config(&opamp_remote_config, &mut running_sub_agents)
                .is_err()
        );

        assert_eq!(running_sub_agents.len(), 1);

        running_sub_agents.stop()
    }

    #[test]
    fn create_sub_agent_wrong_agent_type_from_remote_config() {
        // Sub Agents
        let sub_agents_config = sub_agents_default_config().agents;

        let mut sub_agent_builder = MockSubAgentBuilder::new();
        sub_agent_builder.should_build(2);

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| {
                Err(DynamicConfigValidatorError::from(
                    AgentRepositoryError::NotFound("not found".to_string()),
                ))
            });

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        // all agents on first load
        sa_dynamic_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        let (_opamp_publisher, opamp_consumer) = pub_sub();

        // Create the Agent Control and run Sub Agents
        let agent_control = AgentControl::new(
            None::<MockStartedOpAMPClient>,
            sub_agent_builder,
            Arc::new(sa_dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        let mut running_sub_agents = agent_control
            .build_and_run_sub_agents(&sub_agents_config)
            .unwrap();

        // just one agent, it should remove the infra-agent
        let opamp_remote_config = OpampRemoteConfig::new(
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
            .apply_remote_agent_control_config(&opamp_remote_config, &mut running_sub_agents);

        assert!(apply_remote.is_err());

        running_sub_agents.stop();
    }

    // Invalid configuration should be reported to OpAMP as Failed and the Agent Control should
    // not apply it nor crash execution.
    #[test]
    fn agent_control_invalid_remote_config_should_be_reported_as_failed() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilder::new();
        let mut dynamic_config_store = MockAgentControlDynamicConfigStore::new();
        let mut started_client = MockStartedOpAMPClient::new();
        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let old_sub_agents_config = AgentControlDynamicConfig::default();
        let agent_id = AgentID::new_agent_control_id();
        let opamp_remote_config = OpampRemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                "invalid_yaml_content:{}".to_string(),
            )]))),
        );
        let dynamic_config_validator = MockDynamicConfigValidator::new();

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: opamp_remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        dynamic_config_store.should_load(&old_sub_agents_config);

        // report failed after trying to unserialize
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: opamp_remote_config.hash.get().into_bytes(),
            error_message: "Error applying Agent Control remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct AgentControlDynamicConfig``".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        started_client.should_set_unhealthy();
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            Some(started_client),
            sub_agent_builder,
            Arc::new(dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        agent_control
            .handle_remote_config(opamp_remote_config, &mut running_sub_agents)
            .unwrap();
    }

    #[test]
    fn agent_control_valid_remote_config_should_be_reported_as_applied() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilder::new();
        let mut dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let mut started_client = MockStartedOpAMPClient::new();
        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent-id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        let old_sub_agents_config = AgentControlDynamicConfig {
            agents: HashMap::from([(
                sub_agent_id.clone(),
                SubAgentConfig {
                    agent_type: "namespace/some_agent_type:0.0.1".try_into().unwrap(),
                },
            )]),
            chart_version: None,
        };

        let agent_id = AgentID::new_agent_control_id();
        let opamp_remote_config = OpampRemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                "agents: {}".to_string(),
            )]))),
        );
        let mut dynamic_config_validator = MockDynamicConfigValidator::new();

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: opamp_remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);
        started_client.should_update_effective_config(1);

        // load current sub agents config
        dynamic_config_store.should_load(&old_sub_agents_config);

        // store remote config with empty agents
        let remote_config_values = RemoteConfig::new(
            serde_yaml::from_str("agents: {}").unwrap(),
            opamp_remote_config.hash.clone(),
        );
        dynamic_config_store.should_store(remote_config_values);

        let mut applied_hash = opamp_remote_config.hash.clone();
        applied_hash.update_state(&ConfigState::Applied);
        dynamic_config_store
            .expect_update_hash_state()
            .with(predicate::eq(applied_hash.state()))
            .times(1)
            .returning(|_| Ok(()));

        // Report config status as applied
        let status = RemoteConfigStatus {
            status: Applied as i32,
            last_remote_config_hash: opamp_remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        started_client.should_set_healthy();
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            Some(started_client),
            sub_agent_builder,
            Arc::new(dynamic_config_store),
            UnboundedBroadcast::default(),
            UnboundedBroadcast::default(),
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            AgentControlConfig::default(),
        );

        agent_control
            .handle_remote_config(opamp_remote_config, &mut running_sub_agents)
            .unwrap();
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Events tests
    ////////////////////////////////////////////////////////////////////////////////////

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Agent Control Healthy event
    #[test]
    fn test_config_updated_should_publish_agent_control_healthy() {
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_health(2);
        started_client.should_update_effective_config(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        // load local config
        let sub_agents_config = AgentControlDynamicConfig::default();
        sa_dynamic_config_store.should_load(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let opamp_remote_config = OpampRemoteConfig::new(
            AgentID::new_agent_control_id(),
            remote_config_hash.clone(),
            Some(ConfigurationMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        let yaml_config = serde_yaml::from_str("agents: {}").unwrap();
        let remote_config_values = RemoteConfig::new(yaml_config, remote_config_hash.clone());
        // store remote config
        sa_dynamic_config_store.should_store(remote_config_values);

        // store agent control remote config hash
        remote_config_hash.update_state(&ConfigState::Applied);
        sa_dynamic_config_store
            .expect_update_hash_state()
            .with(predicate::eq(remote_config_hash.state()))
            .times(1)
            .returning(|_| Ok(()));

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
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());

        let event_processor = spawn({
            move || {
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch to allow comparison
                agent.process_events(sub_agents);
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(opamp_remote_config))
            .unwrap();
        sleep(Duration::from_millis(10));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an OpAMP Invalid Config should publish Unhealthy Event
    #[test]
    fn test_invalid_config_should_publish_agent_control_unhealthy() {
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        // set healthy on start processing events
        started_client.should_set_healthy();
        // set unhealthy on invalid config
        started_client.should_set_unhealthy();
        // applying and failed
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidator::new();

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        remote_config_hash.update_state(&ConfigState::Failed {
            error_message: String::from("some error message"),
        });

        let opamp_remote_config =
            OpampRemoteConfig::new(AgentID::new_agent_control_id(), remote_config_hash, None);

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());

        let event_processor = spawn({
            move || {
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch to allow comparison
                agent.process_events(sub_agents);
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(opamp_remote_config))
            .unwrap();

        sleep(Duration::from_millis(10));

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Unhealthy::new(
            String::default(),
            String::from(
                "Error applying Agent Control remote config: remote config error: `config hash: `a-hash` config error: `some error message``",
            ),
        ).into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Receive an StopRequest event should publish AgentControlStopped
    #[test]
    fn test_stop_request_should_publish_agent_control_stopped() {
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        // set healthy on start processing events
        started_client.should_set_healthy();

        let sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let dynamic_config_validator = MockDynamicConfigValidator::new();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::default());

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
        let (_opamp_publisher, opamp_consumer) = pub_sub();

        let event_processor = spawn({
            move || {
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch to allow comparison
                agent.process_events(sub_agents);
            }
        });

        sleep(Duration::from_millis(10));

        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
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
        let sub_agent_builder = MockSubAgentBuilder::new();

        // Agent Control OpAMP
        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_set_health(2);
        started_client.should_update_effective_config(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));

        let mut sa_dynamic_config_store = MockAgentControlDynamicConfigStore::new();

        let mut dynamic_config_validator = MockDynamicConfigValidator::new();
        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        let agent_id = AgentID::new("infra-agent").unwrap();

        // load local config
        let sub_agents_config = AgentControlDynamicConfig {
            agents: HashMap::from([(
                agent_id.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
                },
            )]),
            chart_version: None,
        };

        sa_dynamic_config_store.should_load(&sub_agents_config);

        let mut remote_config_hash = Hash::new("a-hash".to_string());
        let opamp_remote_config = OpampRemoteConfig::new(
            AgentID::new_agent_control_id(),
            remote_config_hash.clone(),
            Some(ConfigurationMap::new(HashMap::from([(
                String::default(),
                String::from("agents: {}"),
            )]))),
        );

        // store remote config
        let yaml_config = serde_yaml::from_str("agents: {}").unwrap();
        let remote_config_values = RemoteConfig::new(yaml_config, remote_config_hash.clone());
        sa_dynamic_config_store.should_store(remote_config_values);

        // store agent control remote config hash
        remote_config_hash.update_state(&ConfigState::Applied);
        sa_dynamic_config_store
            .expect_update_hash_state()
            .with(predicate::eq(remote_config_hash.state()))
            .times(1)
            .returning(|_| Ok(()));

        // the running sub agent that will be stopped
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let (application_event_publisher, application_event_consumer) = pub_sub();
        let (opamp_publisher, opamp_consumer) = pub_sub();
        let mut agent_control_publisher = UnboundedBroadcast::default();
        let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());

        let event_processor = spawn({
            move || {
                let mut agent = AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    Arc::new(sa_dynamic_config_store),
                    agent_control_publisher,
                    UnboundedBroadcast::default(),
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    NoOpResourceCleaner,
                    NoOpUpdater,
                    AgentControlConfig::default(),
                );
                agent.start_time = SystemTime::UNIX_EPOCH; // Patch to allow comparison
                agent.process_events(sub_agents);
            }
        });

        opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(opamp_remote_config))
            .unwrap();
        sleep(Duration::from_millis(10));
        application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // process_events always starts with AgentControlHealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::SubAgentRemoved(agent_id);
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Healthy::default().into(),
            SystemTime::UNIX_EPOCH,
        ));
        let ev = agent_control_consumer.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }
}
