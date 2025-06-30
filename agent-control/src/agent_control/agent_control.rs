use super::config::{
    AgentControlConfig, AgentControlDynamicConfig, SubAgentsMap, sub_agents_difference,
};
use super::config_repository::repository::AgentControlDynamicConfigRepository;
use super::resource_cleaner::ResourceCleaner;
use super::version_updater::updater::VersionUpdater;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config_validator::DynamicConfigValidator;
use crate::agent_control::error::{AgentError, RemoteConfigErrors};
use crate::agent_control::uptime_report::UptimeReporter;
use crate::event::AgentControlInternalEvent;
use crate::event::channel::{EventPublisher, pub_sub};
use crate::event::{
    AgentControlEvent, ApplicationEvent, OpAMPEvent, broadcaster::unbounded::UnboundedBroadcast,
    channel::EventConsumer,
};
use crate::health::health_checker::{HealthChecker, spawn_health_checker};
use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::remote_config::report::report_state;
use crate::opamp::remote_config::{OpampRemoteConfig, OpampRemoteConfigError, hash::ConfigState};
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

pub struct AgentControl<S, O, SL, DV, RC, VU, HC, HCB>
where
    O: StartedClient,
    SL: AgentControlDynamicConfigRepository,
    S: SubAgentBuilder,
    DV: DynamicConfigValidator,
    RC: ResourceCleaner,
    VU: VersionUpdater,
    HC: HealthChecker + Send + 'static,
    HCB: Fn(SystemTime) -> Option<HC>,
{
    pub(super) opamp_client: Option<O>,
    sub_agent_builder: S,
    start_time: SystemTime,
    pub(super) sa_dynamic_config_store: Arc<SL>,
    pub(super) agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    agent_control_internal_consumer: EventConsumer<AgentControlInternalEvent>,
    agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    dynamic_config_validator: DV,
    resource_cleaner: RC,
    version_updater: VU,
    initial_config: AgentControlConfig,
    health_checker_builder: HCB,
}

impl<S, O, SL, DV, RC, VU, HC, HCB> AgentControl<S, O, SL, DV, RC, VU, HC, HCB>
where
    O: StartedClient,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigRepository,
    DV: DynamicConfigValidator,
    RC: ResourceCleaner,
    VU: VersionUpdater,
    HC: HealthChecker + Send + 'static,
    HCB: Fn(SystemTime) -> Option<HC>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opamp_client: Option<O>,
        sub_agent_builder: S,
        start_time: SystemTime,
        sa_dynamic_config_store: Arc<SL>,
        agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
        application_event_consumer: EventConsumer<ApplicationEvent>,
        agent_control_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        dynamic_config_validator: DV,
        resource_cleaner: RC,
        version_updater: VU,
        health_checker_builder: HCB,
        initial_config: AgentControlConfig,
    ) -> Self {
        let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();
        Self {
            opamp_client,
            sub_agent_builder,
            start_time,
            sa_dynamic_config_store,
            agent_control_publisher,
            application_event_consumer,
            agent_control_opamp_consumer,
            agent_control_internal_consumer,
            agent_control_internal_publisher,
            dynamic_config_validator,
            resource_cleaner,
            health_checker_builder,
            version_updater,
            initial_config,
        }
    }

    pub fn run(self) -> Result<(), AgentError> {
        if let Some(opamp_client) = &self.opamp_client {
            match self.sa_dynamic_config_store.get_remote_config() {
                Err(e) => {
                    warn!("Failed getting remote config from the store: {}", e);
                }
                Ok(Some(rc)) => {
                    if !rc.is_applied() {
                        report_state(ConfigState::Applied, rc.hash, opamp_client)?;
                        self.sa_dynamic_config_store
                            .update_state(ConfigState::Applied)?;
                    }
                }
                Ok(None) => {
                    info!("OpAMP enabled but no previous remote configuration found");
                }
            }
            opamp_client.update_effective_config()?
        }

        let _health_checker_thread_context =
            (self.health_checker_builder)(self.start_time).map(|health_checker| {
                debug!("Starting Agent Control health-checker");
                spawn_health_checker(
                    AgentID::new_agent_control_id(),
                    health_checker,
                    self.agent_control_internal_publisher.clone(),
                    self.initial_config.health_check.interval,
                    self.start_time,
                )
            });

        // This update handles scenarios where applying a remote configuration containing an updated Agent Control (AC)
        // was initiated but did not complete successfully, leaving the remote configuration un-stored.
        // In such cases, the AC with the new version starts, reads the previous remote configuration (which specifies the prior version),
        // and rolls back to that version to ensure consistency.
        let _ = self
            .version_updater
            .update(&self.initial_config.dynamic)
            .inspect_err(|err| error!("Error executing Agent Control updater: {err}"));

        info!("Starting the agents supervisor runtime");
        // This is a first-time run and we already read the config earlier, the `initial_config` contains
        // the result as read by the `AgentControlConfigLoader`.
        let running_sub_agents = self.build_and_run_sub_agents(&self.initial_config.dynamic.agents);

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

    /// Returns a collection of started sub agents. In case an agent fails to build an error
    /// is logged and that agent skipped from the list.
    fn build_and_run_sub_agents(
        &self,
        sub_agents: &SubAgentsMap,
    ) -> StartedSubAgents<<S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent> {
        let mut running_sub_agents = StartedSubAgents::default();

        for (agent_id, agent_config) in sub_agents {
            let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));

            match self.sub_agent_builder.build(&agent_identity) {
                Ok(not_started_sub_agent) => {
                    debug!(%agent_id, "Sub agent built");
                    running_sub_agents.insert(agent_identity.id, not_started_sub_agent.run());
                }
                Err(err) => {
                    error!(%agent_id, "Error building agent: {err}");
                }
            }
        }
        running_sub_agents
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
            self.sub_agent_builder.build(agent_identity)?.run(),
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

        let mut current_dynamic_config = self.initial_config.dynamic.clone();

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

                                    match self.handle_remote_config(remote_config, &mut sub_agents,&current_dynamic_config){
                                        Ok(new_dynamic_config)=>{
                                            // A new config has been applied from remote, so we update the current to this.
                                            current_dynamic_config=new_dynamic_config
                                        }
                                        Err(err)=> {
                                            error!(error_msg = %err,"Error processing remote config")
                                        }
                                    };
                                }
                                OpAMPEvent::Connected => self.agent_control_publisher.broadcast(AgentControlEvent::OpAMPConnected),
                                OpAMPEvent::ConnectFailed(error_code, error_message) => self.agent_control_publisher.broadcast(AgentControlEvent::OpAMPConnectFailed(error_code, error_message))
                            }
                        }
                    }
                },
                recv(&self.agent_control_internal_consumer.as_ref()) -> internal_event_res => {
                    match internal_event_res {
                        Err(err) => {
                            debug!("Error receiving Agent Control internal event {err}");
                        },
                        Ok(internal_event) => {
                            match internal_event {
                                AgentControlInternalEvent::HealthUpdated(health) => {
                                    self.report_health(health);
                                },
                            }
                        },
                    }
                }
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
        current_dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<AgentControlDynamicConfig, AgentError> {
        let Some(opamp_client) = &self.opamp_client else {
            unreachable!("got remote config without OpAMP being enabled");
        };

        info!("Applying remote config");
        report_state(
            ConfigState::Applying,
            opamp_remote_config.hash.clone(),
            opamp_client,
        )?;

        match self.validate_apply_store_remote_config(
            &opamp_remote_config,
            sub_agents,
            current_dynamic_config,
        ) {
            Err(err) => {
                let error_message = format!("Error applying Agent Control remote config: {}", err);
                report_state(
                    ConfigState::Failed {
                        error_message: error_message.clone(),
                    },
                    opamp_remote_config.hash,
                    opamp_client,
                )?;
                Err(err)
            }
            Ok(new_dynamic_config) => {
                self.sa_dynamic_config_store
                    .update_state(ConfigState::Applied)?;
                report_state(ConfigState::Applied, opamp_remote_config.hash, opamp_client)?;
                opamp_client.update_effective_config()?;
                Ok(new_dynamic_config)
            }
        }
    }

    #[instrument(skip_all)]
    // apply an agent control remote config
    pub(super) fn validate_apply_store_remote_config(
        &self,
        opamp_remote_config: &OpampRemoteConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        current_dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<AgentControlDynamicConfig, AgentError> {
        // Fail if the remote config has already identified as failed.
        if let Some(err) = opamp_remote_config.state.error_message().cloned() {
            // TODO seems like this error should be sent by the remote config itself
            return Err(OpampRemoteConfigError::InvalidConfig(
                opamp_remote_config.hash.to_string(),
                err,
            )
            .into());
        }

        let remote_config_value = opamp_remote_config.get_unique()?;

        let new_dynamic_config = if remote_config_value.is_empty() {
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
            .validate(&new_dynamic_config)?;

        // The updater is responsible for determining the current version and deciding whether an update is necessary.
        self.version_updater.update(&new_dynamic_config)?;

        // It stores the remote config and then apply it for these reasons:
        // - The apply mechanism does not handle any rollback in case any failure but instead attempts to apply as much as
        //   possible, and in case of failure a partial config keeps running.
        // - It has already been validated so we assume that the possible fails that could happen when applying are recoverable,
        //   and probably happened due to sub-agent OpAMP build errors.
        // - In case of a AC reset , the state will be the same as the current or even better with the config correctly applied.
        // - The effective config will be more similar to the current in execution.
        if !remote_config_value.is_empty() {
            let config = RemoteConfigValues {
                config: YAMLConfig::try_from(remote_config_value.to_string())?,
                hash: opamp_remote_config.hash.clone(),
                state: opamp_remote_config.state.clone(),
            };
            self.sa_dynamic_config_store.store(&config)?;
        }
        // Even if the config was stored and some agents could have been applied, it returns the error so the config is reported
        // as failed, to signal FC that something has gone wrong.
        self.apply_remote_config(
            current_dynamic_config,
            &new_dynamic_config,
            running_sub_agents,
        )?;

        Ok(new_dynamic_config)
    }

    /// Applies the remote configuration for agents.
    /// It will create new agents, recreate existing ones with changed configuration, and remove those
    /// that are no longer present in the new configuration.
    /// Attempts to apply as much of the configuration as possible. If an agent fails to be recreated, updated, or removed,
    /// that specific agent will be skipped, but the rest of the configuration changes will still be applied.
    pub(super) fn apply_remote_config(
        &self,
        current_dynamic_config: &AgentControlDynamicConfig,
        new_dynamic_config: &AgentControlDynamicConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let mut errors = RemoteConfigErrors::default();

        for (agent_id, agent_config) in &new_dynamic_config.agents {
            let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));

            let apply_result = match current_dynamic_config.agents.get(agent_id) {
                Some(old_sub_agent_config) if old_sub_agent_config == agent_config => {
                    debug!(%agent_id, "Retaining the existing running SubAgent as its configuration remains unchanged");
                    Ok(())
                }
                Some(_) => {
                    info!(%agent_id, "Recreating SubAgent");
                    self.recreate_sub_agent(&agent_identity, running_sub_agents)
                }
                None => {
                    info!(%agent_id, "Creating SubAgent");
                    self.build_and_run_sub_agent(&agent_identity, running_sub_agents)
                }
            };

            if let Err(err) = apply_result {
                errors.push(agent_id.clone(), err);
            };
        }

        let sub_agents_to_remove =
            sub_agents_difference(&current_dynamic_config.agents, &new_dynamic_config.agents);

        for (agent_id, agent_config) in sub_agents_to_remove {
            if let Err(err) = running_sub_agents.stop_and_remove(agent_id) {
                errors.push(agent_id.clone(), err.into());
            };

            if let Err(err) = self
                .resource_cleaner
                .clean(agent_id, &agent_config.agent_type)
            {
                errors.push(agent_id.clone(), err.into());
            };

            self.agent_control_publisher
                .broadcast(AgentControlEvent::SubAgentRemoved(agent_id.clone()));
        }

        if !errors.is_empty() {
            Err(AgentError::ApplyingRemoteConfig(errors))
        } else {
            Ok(())
        }
    }

    fn report_health(&self, health: HealthWithStartTime) {
        if let Some(handle) = &self.opamp_client {
            debug!(
                is_healthy = health.is_healthy().to_string(),
                "Sending agent-control health"
            );

            let _ = handle.set_health(health.clone().into()).inspect_err(|err| {
                error!("Error reporting health for Agent Control: {err}");
            });
        }
        self.agent_control_publisher
            .broadcast(AgentControlEvent::HealthUpdated(health));
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::AgentControl;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::tests::{
        infra_identity, nrdot_identity, sub_agents_infra_and_nrdot, sub_agents_nrdot,
    };
    use crate::agent_control::config::{
        AgentControlConfig, AgentControlDynamicConfig, SubAgentConfig,
    };
    use crate::agent_control::config_repository::repository::AgentControlDynamicConfigRepository;
    use crate::agent_control::config_repository::repository::tests::InMemoryAgentControlDynamicConfigRepository;
    use crate::agent_control::config_validator::tests::TestDynamicConfigValidator;
    use crate::agent_control::error::AgentError;
    use crate::agent_control::resource_cleaner::tests::MockResourceCleaner;
    use crate::agent_control::version_updater::updater::tests::MockVersionUpdater;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::event::broadcaster::unbounded::UnboundedBroadcast;
    use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
    use crate::event::{AgentControlEvent, ApplicationEvent, OpAMPEvent};
    use crate::health::health_checker::Unhealthy;
    use crate::health::health_checker::tests::MockHealthCheck;
    use crate::health::with_start_time::HealthWithStartTime;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::{ConfigurationMap, OpampRemoteConfig};
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::identity::AgentIdentity;
    use crate::sub_agent::tests::MockSubAgentBuilder;
    use crate::sub_agent::tests::{MockNotStartedSubAgent, MockStartedSubAgent};
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::ConfigRepository;
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use mockall::{Sequence, predicate};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::{Duration, SystemTime};

    /// Type to represent the testing implementation of Agent control
    type TestAgentControl = AgentControl<
        MockSubAgentBuilder,
        MockStartedOpAMPClient,
        InMemoryAgentControlDynamicConfigRepository,
        TestDynamicConfigValidator,
        MockResourceCleaner,
        MockVersionUpdater,
        MockHealthCheck,
        fn(SystemTime) -> Option<MockHealthCheck>,
    >;

    /// Holds test data to interact with AC events and perform particular assertions in tests
    struct TestData {
        channels: Channels,
        dyn_config_store: Arc<InMemoryAgentControlDynamicConfigRepository>,
    }

    struct Channels {
        app_publisher: EventPublisher<ApplicationEvent>,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        broadcast_subscriber: EventConsumer<AgentControlEvent>,
    }

    impl TestData {
        /// Executes the provided function using the stored remote configuration. It panics if there is any error
        /// receiving the remote configuration or there is no remote configuration.
        fn assert_stored_remote_config<F: Fn(RemoteConfig)>(&self, f: F) {
            let config = self
                .dyn_config_store
                .values_repository
                .get_remote_config(&AgentID::new_agent_control_id())
                .expect("Error getting the remote config for Agent control")
                .expect("No remote config found for Agent Control");
            f(config)
        }

        /// Builds a remote configuration for AC corresponding to the provided string
        fn build_ac_remote_config(&self, s: &str) -> OpampRemoteConfig {
            // Build the hash, import locally in order to avoid collisions
            use std::hash::{Hash as StdHash, Hasher};
            let mut hasher = std::hash::DefaultHasher::new();
            s.to_string().hash(&mut hasher);
            let hash = Hash::from(hasher.finish().to_string());

            OpampRemoteConfig::new(
                AgentID::new_agent_control_id(),
                hash,
                ConfigState::Applying,
                Some(ConfigurationMap::new(HashMap::from([(
                    "".to_string(),
                    s.to_string(),
                )]))),
            )
        }
    }

    /// None builder for [MockHealthCheck]
    const NONE_MOCK_HEALTH_CHECKER_BUILDER: fn(SystemTime) -> Option<MockHealthCheck> = |_| None;

    impl TestAgentControl {
        /// Builds an Agent Control for testing purposes with default mocks and returns the corresponding [TestData]
        /// to easy events iteration and assertions.
        fn setup() -> (TestData, Self) {
            let sa_dynamic_config_store =
                Arc::new(InMemoryAgentControlDynamicConfigRepository::default());
            let started_client = MockStartedOpAMPClient::new();
            let dynamic_config_validator = TestDynamicConfigValidator { valid: true };
            let sub_agent_builder = MockSubAgentBuilder::new();
            let (application_event_publisher, application_event_consumer) = pub_sub();
            let (opamp_publisher, opamp_consumer) = pub_sub();
            let mut agent_control_publisher = UnboundedBroadcast::default();
            let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
            let resource_cleaner = MockResourceCleaner::new();
            let version_updater = MockVersionUpdater::new();

            let agent_control = {
                AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    SystemTime::UNIX_EPOCH,
                    sa_dynamic_config_store.clone(),
                    agent_control_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    dynamic_config_validator,
                    resource_cleaner,
                    version_updater,
                    NONE_MOCK_HEALTH_CHECKER_BUILDER,
                    AgentControlConfig::default(),
                )
            };
            let test_data = TestData {
                channels: Channels {
                    app_publisher: application_event_publisher,
                    opamp_publisher,
                    broadcast_subscriber: agent_control_consumer,
                },
                dyn_config_store: sa_dynamic_config_store,
            };

            (test_data, agent_control)
        }

        /// Sets OpAMP mock expectations (helper to easy handling the option)
        fn set_opamp_expectations<F: Fn(&mut MockStartedOpAMPClient)>(&mut self, f: F) {
            if let Some(opamp_client) = self.opamp_client.as_mut() {
                f(opamp_client);
            }
        }

        /// Sets if configuration should be valid or not
        fn set_dynamic_config_valid(&mut self, valid: bool) {
            self.dynamic_config_validator = TestDynamicConfigValidator { valid }
        }

        /// Sets and stores the initial configuration corresponding to the provided string
        fn set_initial_config(&mut self, config: String) {
            let as_yaml = YAMLConfig::try_from(config).unwrap();
            // store as local
            self.sa_dynamic_config_store
                .values_repository
                .store_local(&AgentID::new_agent_control_id(), &as_yaml)
                .unwrap();
            // load the dynamic part
            let dyn_config = self.sa_dynamic_config_store.load().unwrap();
            // sets it up as initial config for AC
            self.initial_config = AgentControlConfig {
                dynamic: dyn_config,
                ..Default::default()
            };
        }

        /// Sets a resource cleaner with "no-op" expectations
        fn set_noop_resource_cleaner(&mut self) {
            self.resource_cleaner
                .expect_clean()
                .returning(|_, _| Ok(()));
        }

        /// Sets a mock with "no-op" expectations
        fn set_noop_updater(&mut self) {
            self.version_updater = MockVersionUpdater::new_no_op();
        }

        /// Set expectations in sequence for each provided [AgentIdentity] to be _cleaned_ by the resource cleaner.
        fn expect_resource_clean_in_sequence(&mut self, identities: Vec<AgentIdentity>) {
            let mut seq = Sequence::new();
            for identity in identities {
                self.resource_cleaner
                    .expect_clean()
                    .once()
                    .in_sequence(&mut seq)
                    .with(
                        predicate::eq(identity.id),
                        predicate::eq(identity.agent_type_id),
                    )
                    .returning(|_, _| Ok(()));
            }
        }

        /// Set expectations for each provided [AgentIdentity] to be built by the sub-agent builder.
        /// Each sub-agent build will expect to run and stop.
        fn set_sub_agent_build_success(&mut self, identities: Vec<AgentIdentity>) {
            for identity in identities {
                self.sub_agent_builder
                    .expect_build()
                    .once()
                    .with(predicate::eq(identity))
                    .returning(|_| {
                        let mut not_started = MockNotStartedSubAgent::new();
                        not_started.expect_run().once().returning(|| {
                            let mut started = MockStartedSubAgent::new();
                            started.expect_stop().once().returning(|| Ok(()));
                            started
                        });
                        Ok(not_started)
                    });
            }
        }

        /// Set expectations for each provided [AgentIdentity] to be built by the sub-agent builder.
        /// Each sub-agent build will fail.
        #[allow(dead_code)] // TODO: it will be used soon
        fn set_sub_agent_build_fail(&mut self, identities: Vec<AgentIdentity>) {
            for identity in identities {
                self.sub_agent_builder
                    .expect_build()
                    .once()
                    .with(predicate::eq(identity))
                    .returning(|_| {
                        Err(SubAgentBuilderError::UnsupportedK8sObject(
                            "some error".to_string(),
                        ))
                    });
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Bootstrap Agents Tests
    ////////////////////////////////////////////////////////////////////////////////////

    #[test]
    fn bootstrap_empty_agents() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config("agents: {}\n".to_string());

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn bootstrap_multiple_agents_local() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.initial_config = AgentControlConfig {
            dynamic: sub_agents_infra_and_nrdot(),
            ..Default::default()
        };
        agent_control.set_sub_agent_build_success(vec![nrdot_identity(), infra_identity()]);

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn bootstrap_agents_from_remote_config_applied() {
        // TODO
    }

    #[test]
    fn bootstrap_agents_from_remote_config_failed() {
        // TODO
    }

    #[test]
    fn bootstrap_with_failing_agents() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.initial_config = AgentControlConfig {
            dynamic: sub_agents_infra_and_nrdot(),
            ..Default::default()
        };
        agent_control.set_sub_agent_build_success(vec![nrdot_identity()]);
        agent_control.set_sub_agent_build_fail(vec![infra_identity()]);

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();
        // AC will start and run but only with one agent
        assert!(agent_control.run().is_ok())
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Events
    ////////////////////////////////////////////////////////////////////////////////////

    #[test]
    // This test makes sure that after receiving an "OpAMPEvent::Connected" the AC reports the corresponding
    // broadcast event
    fn receive_opamp_connected() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        let sub_agents = StartedSubAgents::from(HashMap::default());
        // Start processing events in a different thread
        let event_processor = spawn({
            move || {
                agent_control.process_events(sub_agents);
            }
        });

        // When OpAMPEvent::Connected is received
        t.channels
            .opamp_publisher
            .publish(OpAMPEvent::Connected)
            .unwrap();

        // AC should publish the corresponding broadcast event
        let expected = AgentControlEvent::OpAMPConnected;
        let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        // Publish the event to stop the application
        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();
        assert!(event_processor.join().is_ok());
    }

    #[test]
    // This tests makes sure that after receiving an "OpAMPEvent::ConnectFailed" the AC reports the corresponding
    // broadcast event
    fn receive_opamp_connect_failed() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        let sub_agents = StartedSubAgents::from(HashMap::default());
        // Start processing events in a different thread
        let event_processor = spawn({
            move || {
                agent_control.process_events(sub_agents);
            }
        });

        // When OpAMPEvent::ConnectFailed is received
        t.channels
            .opamp_publisher
            .publish(OpAMPEvent::ConnectFailed(
                Some(500),
                "Internal error".to_string(),
            ))
            .unwrap();

        // AC should publish the corresponding broadcast event
        let expected =
            AgentControlEvent::OpAMPConnectFailed(Some(500), "Internal error".to_string());
        let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        // Publish the event to stop the application
        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok())
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Remote Config Tests
    ////////////////////////////////////////////////////////////////////////////////////

    #[test]
    fn receive_opamp_remote_config() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();
        // Set initial config with nr-dot + infra
        agent_control.initial_config = AgentControlConfig {
            dynamic: sub_agents_infra_and_nrdot(),
            ..Default::default()
        };
        agent_control.set_opamp_expectations(|client| {
            client
                .expect_set_remote_config_status()
                .times(2)
                .returning(|_| Ok(()));
            client.should_update_effective_config(2);
            client.should_stop(1);
        });

        // it should build two subagents: nrdot + infra-agent
        agent_control.set_sub_agent_build_success(vec![infra_identity(), nrdot_identity()]);

        let running_agent_control = spawn({
            move || {
                // two agents in the supervisor group
                agent_control.run()
            }
        });

        let opamp_remote_config = t.build_ac_remote_config(
            r#"
agents:
  infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.1"
"#,
        );
        t.channels
            .opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(
                opamp_remote_config.clone(),
            ))
            .unwrap();
        sleep(Duration::from_millis(500));

        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(running_agent_control.join().is_ok());

        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert_eq!(config.state, ConfigState::Applied);
        });
    }

    #[test]
    /// Checks that the resource cleaner is called as expected when the list of agents change due to remote config
    /// updates.
    fn create_stop_sub_agents_from_remote_config() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_updater();
        // Sub Agents
        let sub_agents_config = sub_agents_infra_and_nrdot().agents;

        // Build initial agents (infra + nr-dot) and then build infra again when adding it back after removing.
        agent_control.set_sub_agent_build_success(vec![
            infra_identity(),
            nrdot_identity(),
            infra_identity(),
        ]);

        // First cleans-up the infra-agent then, cleans up nrdot
        agent_control.expect_resource_clean_in_sequence(vec![infra_identity(), nrdot_identity()]);

        let mut running_sub_agents = agent_control.build_and_run_sub_agents(&sub_agents_config);

        // just one agent, it should remove the infra-agent
        let opamp_remote_config_nrdot = t.build_ac_remote_config(
            r#"
agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#,
        );
        assert_eq!(running_sub_agents.len(), 2);

        agent_control
            .validate_apply_store_remote_config(
                &opamp_remote_config_nrdot,
                &mut running_sub_agents,
                &sub_agents_infra_and_nrdot(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra-agent sub_agent
        let opamp_remote_config_infra = t.build_ac_remote_config(
            r#"
agents:
  infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure:0.0.1
"#,
        );

        agent_control
            .validate_apply_store_remote_config(
                &opamp_remote_config_infra,
                &mut running_sub_agents,
                &sub_agents_nrdot(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        running_sub_agents.stop()
    }

    #[test]
    fn agent_control_fails_if_resource_cleaning_fails() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.opamp_client = None;
        agent_control.set_noop_updater();

        // Sub Agents
        let sub_agents_config = sub_agents_infra_and_nrdot().agents;

        agent_control.set_sub_agent_build_success(vec![infra_identity(), nrdot_identity()]);

        agent_control.expect_resource_clean_in_sequence(vec![infra_identity()]);

        let mut running_sub_agents = agent_control.build_and_run_sub_agents(&sub_agents_config);

        // just one agent, it should remove the infra-agent
        let opamp_remote_config_nrdot = t.build_ac_remote_config(
            r#"
agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#,
        );

        assert_eq!(running_sub_agents.len(), 2);

        agent_control
            .validate_apply_store_remote_config(
                &opamp_remote_config_nrdot,
                &mut running_sub_agents,
                &sub_agents_infra_and_nrdot(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        running_sub_agents.stop()
    }

    #[test]
    fn create_sub_agent_wrong_agent_type_from_remote_config() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        // Sub Agents
        let sub_agents_config = sub_agents_infra_and_nrdot().agents;

        agent_control.set_sub_agent_build_success(vec![infra_identity(), nrdot_identity()]);

        agent_control.set_dynamic_config_valid(false); // Expect invalid configuration

        let mut running_sub_agents = agent_control.build_and_run_sub_agents(&sub_agents_config);

        // just one agent, it should remove the infra-agent
        let opamp_remote_config_invalid_type = t.build_ac_remote_config(
            r#"
agents:
  nrdot:
    agent_type: newrelic/invented-agent-type:0.0.1

"#,
        );

        assert_eq!(running_sub_agents.len(), 2);

        let apply_remote = agent_control.validate_apply_store_remote_config(
            &opamp_remote_config_invalid_type,
            &mut running_sub_agents,
            &AgentControlConfig::default().dynamic,
        );

        assert!(apply_remote.is_err());

        running_sub_agents.stop();
    }

    // Invalid configuration should be reported to OpAMP as Failed and the Agent Control should
    // not apply it nor crash execution.
    #[test]
    fn agent_control_invalid_remote_config_should_be_reported_as_failed() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();
        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let current_sub_agents_config = AgentControlDynamicConfig::default();
        let opamp_remote_config = t.build_ac_remote_config("invalid_yaml_content:{}");
        //Expectations
        agent_control.set_opamp_expectations( |client| {
            // Report config status as applying
            let status = RemoteConfigStatus {
                status: Applying as i32,
                last_remote_config_hash: opamp_remote_config.hash.to_string().into_bytes(),
                error_message: "".to_string(),
            };
            client.should_set_remote_config_status(status);

            // report failed after trying to unserialize
            let status = RemoteConfigStatus {
                status: Failed as i32,
                last_remote_config_hash: opamp_remote_config.hash.to_string().into_bytes(),
                error_message: "Error applying Agent Control remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct AgentControlDynamicConfig``".to_string(),
            };
            client.should_set_remote_config_status(status);
        });

        let err = agent_control
            .handle_remote_config(
                opamp_remote_config,
                &mut running_sub_agents,
                &current_sub_agents_config,
            )
            .unwrap_err();

        assert_matches!(err, AgentError::ConfigResolve(_))
    }

    #[test]
    fn agent_control_valid_remote_config_should_be_reported_as_applied() {
        // TODO: improve description
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent-id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        // local config
        let current_sub_agents_config = AgentControlDynamicConfig {
            agents: HashMap::from([(
                sub_agent_id.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from("namespace/some_agent_type:0.0.1").unwrap(),
                },
            )]),
            ..Default::default()
        };

        let opamp_remote_config = t.build_ac_remote_config("agents: {}");

        agent_control.set_opamp_expectations(|client| {
            // Report config status as applying
            let status = RemoteConfigStatus {
                status: Applying as i32,
                last_remote_config_hash: opamp_remote_config.hash.to_string().into_bytes(),
                error_message: "".to_string(),
            };
            client.should_set_remote_config_status(status);
            // Report config status as Applied
            let status = RemoteConfigStatus {
                status: Applied as i32,
                last_remote_config_hash: opamp_remote_config.hash.to_string().into_bytes(),
                error_message: "".to_string(),
            };
            client.should_set_remote_config_status(status);
            // Update effective config
            client.should_update_effective_config(1);
        });

        agent_control
            .handle_remote_config(
                opamp_remote_config.clone(),
                &mut running_sub_agents,
                &current_sub_agents_config,
            )
            .unwrap();

        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert_eq!(config.state, ConfigState::Applied);
        });
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Agent Control Events tests
    ////////////////////////////////////////////////////////////////////////////////////

    // Health Checker events are correctly published
    #[test]
    fn test_health_checker_events() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_opamp_expectations(|client| {
            // Set unhealthy at least twice
            client
                .expect_set_health()
                .withf(|health| !health.healthy)
                .times(2..)
                .returning(|_| Ok(()));
            client.should_update_effective_config(1); // Update effective config when AC starts
            client.should_stop(1);
        });

        // Patch the default health-checker builder and interval
        agent_control.health_checker_builder = |_| Some(MockHealthCheck::new_unhealthy()); // The health-check will always return unhealthy
        agent_control.initial_config.health_check.interval = Duration::from_millis(20).into();

        let event_processor = spawn(move || agent_control.run());

        // Leave some time for the health-checker to execute (every 20ms)
        sleep(Duration::from_millis(100));

        // Send the stop signal
        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        // Agent control broadcast health information
        // It should report at least twice
        let messages_count = t.channels.broadcast_subscriber.as_ref().len();
        assert!(messages_count > 2);

        // The health-checker should report Unhealthy
        let expected = AgentControlEvent::HealthUpdated(HealthWithStartTime::new(
            Unhealthy::new(String::default()).into(),
            SystemTime::UNIX_EPOCH,
        ));

        // The latest message will be StopRequested
        for _ in 0..(messages_count - 1) {
            let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
            assert_eq!(expected, ev);
        }
    }

    // Receive an StopRequest event should publish AgentControlStopped
    #[test]
    fn test_stop_request_should_publish_agent_control_stopped() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        let sub_agents = StartedSubAgents::from(HashMap::default());
        let event_processor = spawn({
            move || {
                agent_control.process_events(sub_agents);
            }
        });

        sleep(Duration::from_millis(10));

        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        let expected = AgentControlEvent::AgentControlStopped;
        let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
        assert_eq!(expected, ev);
    }

    // Having one running sub agent, receive a valid config with no agents
    // and we assert on Agent Control Healthy event
    // And it should publish SubAgentRemoved
    #[test]
    fn test_removing_a_sub_agent_should_publish_sub_agent_removed() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            // applying and applied
            client
                .expect_set_remote_config_status()
                .times(2)
                .returning(|_| Ok(()));
        });

        let agent_id = AgentID::new("infra-agent").unwrap();

        // local config
        let config = format!(
            r#"
agents:
  {agent_id}:
    agent_type: "namespace/some-agent-type:0.0.1"
"#
        );
        agent_control.set_initial_config(config);

        let remote_config_content = "agents: {}";
        let opamp_remote_config = t.build_ac_remote_config(remote_config_content);

        // the running sub agent that will be stopped
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.should_stop();

        // the running sub agents
        let sub_agents = StartedSubAgents::from(HashMap::from([(agent_id.clone(), sub_agent)]));

        let event_processor = spawn({
            move || {
                agent_control.process_events(sub_agents);
            }
        });

        t.channels
            .opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(
                opamp_remote_config.clone(),
            ))
            .unwrap();
        sleep(Duration::from_millis(100));
        t.channels
            .app_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        assert!(event_processor.join().is_ok());

        let expected = AgentControlEvent::SubAgentRemoved(agent_id);
        let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert_eq!(config.state, ConfigState::Applied);
            let expected_yaml_config = serde_yaml::from_str(remote_config_content).unwrap();
            assert_eq!(config.config, expected_yaml_config)
        });
    }
}
