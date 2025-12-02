pub mod agent_id;
pub mod config;
pub mod config_repository;
pub mod config_validator;
pub mod defaults;
pub mod error;
mod health_checker;
pub mod http_server;
pub mod pid_cache;
pub mod resource_cleaner;
pub mod run;
pub mod uptime_report;
pub mod version_updater;

use crate::agent_control::defaults::AGENT_CONTROL_ID;
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::event::{
    AgentControlEvent, ApplicationEvent, OpAMPEvent, broadcaster::unbounded::UnboundedBroadcast,
    channel::EventConsumer,
};
use crate::health::health_checker::{HealthChecker, spawn_health_checker};
use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::remote_config::report::report_state;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::{OpampRemoteConfig, OpampRemoteConfigError, hash::ConfigState};
use crate::sub_agent::{
    NotStartedSubAgent, SubAgentBuilder, collection::StartedSubAgents, identity::AgentIdentity,
};
use crate::values::config::RemoteConfig as RemoteConfigValues;
use crate::values::yaml_config::YAMLConfig;
use crate::version_checker::handler::set_agent_description_version;
use agent_id::AgentID;
use config::{AgentControlConfig, AgentControlDynamicConfig, SubAgentsMap, sub_agents_difference};
use config_repository::repository::AgentControlDynamicConfigRepository;
use config_validator::DynamicConfigValidator;
use crossbeam::channel::never;
use crossbeam::select;
use error::{AgentControlError, BuildingSubagentErrors};
use opamp_client::StartedClient;
use resource_cleaner::ResourceCleaner;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info, info_span, instrument, trace, warn};
use uptime_report::UptimeReporter;
use version_updater::updater::VersionUpdater;

/// Type alias for a [crate::sub_agent::StartedSubAgent] corresponding to a [SubAgentBuilder].
type BuilderStartedSubAgent<S> =
    <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent;

pub struct AgentControl<S, O, SL, RV, DV, RC, VU, HC, HCB>
where
    O: StartedClient,
    SL: AgentControlDynamicConfigRepository,
    S: SubAgentBuilder,
    RV: RemoteConfigValidator,
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
    remote_config_validator: RV,
    dynamic_config_validator: DV,
    resource_cleaner: RC,
    version_updater: VU,
    initial_config: AgentControlConfig,
    health_checker_builder: HCB,
}

impl<S, O, SL, RV, DV, RC, VU, HC, HCB> AgentControl<S, O, SL, RV, DV, RC, VU, HC, HCB>
where
    O: StartedClient,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigRepository,
    RV: RemoteConfigValidator,
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
        agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
        agent_control_internal_consumer: EventConsumer<AgentControlInternalEvent>,
        remote_config_validator: RV,
        dynamic_config_validator: DV,
        resource_cleaner: RC,
        version_updater: VU,
        health_checker_builder: HCB,
        initial_config: AgentControlConfig,
    ) -> Self {
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
            remote_config_validator,
            dynamic_config_validator,
            resource_cleaner,
            health_checker_builder,
            version_updater,
            initial_config,
        }
    }

    pub fn run(self) -> Result<(), AgentControlError> {
        let ac_startup_span = info_span!("start_agent_control", id = AGENT_CONTROL_ID);
        let _ac_startup_span_guard = ac_startup_span.enter();
        info!("Starting the agents supervisor runtime");
        // This is a first-time run and we already read the config earlier, the `initial_config` contains
        // the result as read by the `AgentControlConfigLoader`.
        let (running_sub_agents, build_and_start_result) =
            self.build_and_run_sub_agents(&self.initial_config.dynamic.agents);

        // Get the state corresponding to
        let build_and_start_config_state = if let Err(err) = build_and_start_result {
            let error_message = format!("Failed to build and start agents: {err}");
            error!(error_message);
            ConfigState::Failed { error_message }
        } else {
            ConfigState::Applied
        };

        if let Some(opamp_client) = &self.opamp_client {
            match self.sa_dynamic_config_store.get_remote_config() {
                Err(e) => {
                    warn!("Failed getting remote config from the store: {}", e);
                }
                Ok(Some(rc)) => {
                    if rc.state != build_and_start_config_state {
                        report_state(build_and_start_config_state.clone(), rc.hash, opamp_client)?;
                        self.sa_dynamic_config_store
                            .update_state(build_and_start_config_state)?;
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
                    AgentID::AgentControl,
                    health_checker,
                    self.agent_control_internal_publisher.clone(),
                    self.initial_config.health_check.interval,
                    self.initial_config.health_check.initial_delay,
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

        info!("Agents supervisor runtime successfully started");
        drop(_ac_startup_span_guard); // The span representing agent start finishes before entering in the `process_events` loop. Otherwise the span would be open while Agent Control runs.

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
        running_sub_agents: &mut StartedSubAgents<BuilderStartedSubAgent<S>>,
    ) -> Result<(), AgentControlError> {
        running_sub_agents.stop_and_remove(&agent_identity.id)?;
        self.build_and_run_sub_agent(agent_identity, running_sub_agents)
    }

    /// Returns a tuple containing a collection of started sub-agents and a Result containing information about the
    /// result. The Result will be Ok if all agents where built successfully and will contain an error informing of
    /// the agents with errors otherwise.
    fn build_and_run_sub_agents(
        &self,
        sub_agents: &SubAgentsMap,
    ) -> (
        StartedSubAgents<BuilderStartedSubAgent<S>>,
        Result<(), AgentControlError>,
    ) {
        let mut running_sub_agents = StartedSubAgents::default();
        let mut errors = BuildingSubagentErrors::default();

        for (agent_id, agent_config) in sub_agents {
            let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));

            match self.sub_agent_builder.build(&agent_identity) {
                Ok(not_started_sub_agent) => {
                    debug!(%agent_id, "Sub agent built");
                    running_sub_agents.insert(agent_identity.id, not_started_sub_agent.run());
                }
                Err(err) => {
                    debug!(%agent_id, "Error building sub agent");
                    errors.push(agent_id.clone(), err.into());
                }
            }
        }
        if errors.is_empty() {
            (running_sub_agents, Ok(()))
        } else {
            (
                running_sub_agents,
                Err(AgentControlError::BuildingSubagents(errors)),
            )
        }
    }

    // runs and adds into the sub_agents collection the given agent
    fn build_and_run_sub_agent(
        &self,
        agent_identity: &AgentIdentity,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentControlError> {
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
                    let span = info_span!("process_fleet_event", id=AGENT_CONTROL_ID);
                    let _span_guard = span.enter();
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

                                    match self.handle_remote_config(remote_config, &mut sub_agents, &current_dynamic_config) {
                                        Ok(new_dynamic_config) => {
                                            // A new config has been applied from remote, so we update the current to this.
                                            current_dynamic_config = new_dynamic_config
                                        }
                                        Err(err) => {
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
                    let span = info_span!("process_event", id=AGENT_CONTROL_ID);
                    let _span_guard = span.enter();
                    match internal_event_res {
                        Err(err) => {
                            debug!("Error receiving Agent Control internal event {err}");
                        },
                        Ok(internal_event) => {
                            match internal_event {
                                AgentControlInternalEvent::HealthUpdated(health) => {
                                    self.report_health(health);
                                },
                                AgentControlInternalEvent::AgentControlCdVersionUpdated(cd_version) => {
                                    let _ = self.opamp_client.as_ref().map(|c| set_agent_description_version(
                                        c,
                                        cd_version,
                                    )
                                    .inspect_err(|e| error!(error = %e, select_arm = "agent_control_internal_consumer", "processing version message")));
                                },
                            }
                        },
                    }
                }
                recv(self.application_event_consumer.as_ref()) -> _agent_control_event => {
                    let span = info_span!("process_application_event", id=AGENT_CONTROL_ID);
                    let _span_guard = span.enter();
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
    ) -> Result<AgentControlDynamicConfig, AgentControlError> {
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
            // Remote config partially applied, the config was stored so it needs to be updated to fail state.
            Err(AgentControlError::BuildingSubagents(err)) => {
                let error_message =
                    format!("Error applying Agent Control remote config for some agents: {err}");
                let config_state = ConfigState::Failed { error_message };
                self.sa_dynamic_config_store
                    .update_state(config_state.clone())?;
                report_state(config_state, opamp_remote_config.hash, opamp_client)?;
                opamp_client.update_effective_config()?;
                Err(AgentControlError::BuildingSubagents(err))
            }
            // Remote config failed to apply, the config was not stored.
            Err(err) => {
                let error_message = format!("Error applying Agent Control remote config: {err}");
                let config_state = ConfigState::Failed { error_message };
                report_state(config_state, opamp_remote_config.hash, opamp_client)?;
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

    pub(super) fn validate_apply_store_remote_config(
        &self,
        opamp_remote_config: &OpampRemoteConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        current_dynamic_config: &AgentControlDynamicConfig,
    ) -> Result<AgentControlDynamicConfig, AgentControlError> {
        // Fail if the remote config has already identified as failed.
        if let Some(err) = opamp_remote_config.state.error_message().cloned() {
            // TODO seems like this error should be sent by the remote config itself
            return Err(OpampRemoteConfigError::InvalidConfig(
                opamp_remote_config.hash.to_string(),
                err,
            )
            .into());
        }

        self.remote_config_validator
            .validate(
                &AgentIdentity::new_agent_control_identity(),
                opamp_remote_config,
            )
            .map_err(|err| AgentControlError::RemoteConfigValidator(err.to_string()))?;

        let remote_config_value = opamp_remote_config.get_default()?;

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
            .validate(&new_dynamic_config)
            .map_err(|err| AgentControlError::RemoteConfigValidator(err.to_string()))?;

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
        self.apply_remote_config_agents(
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
    pub(super) fn apply_remote_config_agents(
        &self,
        current_dynamic_config: &AgentControlDynamicConfig,
        new_dynamic_config: &AgentControlDynamicConfig,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentControlError> {
        let mut errors = BuildingSubagentErrors::default();

        for (agent_id, agent_config) in &new_dynamic_config.agents {
            let agent_identity = AgentIdentity::from((agent_id, &agent_config.agent_type));

            let apply_result = match current_dynamic_config.agents.get(agent_id) {
                Some(old_sub_agent_config) if old_sub_agent_config == agent_config => {
                    debug!(%agent_id, "Retaining the existing running SubAgent as its type remains unchanged");
                    Ok(())
                }
                Some(old_sub_agent_config) => {
                    info!(%agent_id, "Recreating SubAgent");
                    self.recreate_sub_agent(&agent_identity, running_sub_agents)?;
                    self.resource_cleaner
                        .clean(agent_id, &old_sub_agent_config.agent_type)?;
                    Ok(())
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
            Err(AgentControlError::BuildingSubagents(errors))
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
    use super::AgentControl;
    use super::agent_id::AgentID;
    use super::config::{AgentControlConfig, AgentControlDynamicConfig};
    use super::config_repository::repository::AgentControlDynamicConfigRepository;
    use super::config_repository::repository::tests::InMemoryAgentControlDynamicConfigRepository;
    use super::config_validator::tests::TestDynamicConfigValidator;
    use super::error::AgentControlError;
    use super::resource_cleaner::tests::MockResourceCleaner;
    use super::version_updater::updater::UpdaterError;
    use super::version_updater::updater::tests::MockVersionUpdater;
    use crate::agent_control::health_checker::AgentControlHealthCheckerConfig;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::event::broadcaster::unbounded::UnboundedBroadcast;
    use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
    use crate::event::{AgentControlEvent, ApplicationEvent, OpAMPEvent};
    use crate::health::health_checker::Unhealthy;
    use crate::health::health_checker::tests::MockHealthCheck;
    use crate::health::with_start_time::HealthWithStartTime;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::validators::tests::TestRemoteConfigValidator;
    use crate::opamp::remote_config::{AGENT_CONFIG_PREFIX, ConfigurationMap, OpampRemoteConfig};
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
    use rstest::rstest;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::{Duration, SystemTime};

    /// Type to represent the testing implementation of Agent control
    type TestAgentControl = AgentControl<
        MockSubAgentBuilder,
        MockStartedOpAMPClient,
        InMemoryAgentControlDynamicConfigRepository,
        TestRemoteConfigValidator,
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
        /// Simple Agent Control configuration containing only one agent
        pub const SINGLE_AGENT_CONFIG: &str = r#"
agents:
  id1:
    agent_type: "newrelic/example:0.0.1"
        "#;

        /// Simple Agent Control configuration containing two agents
        pub const TWO_AGENTS_CONFIG: &str = r#"
agents:
  id1:
    agent_type: "newrelic/example.a:1.2.3"
  id2:
    agent_type: "newrelic/example.b:3.2.1"
        "#;

        fn two_agents_first_agent_identity(&self) -> AgentIdentity {
            self.identities(vec![("id1", "newrelic/example.a:1.2.3")])[0].clone()
        }

        fn two_agents_second_agent_identity(&self) -> AgentIdentity {
            self.identities(vec![("id2", "newrelic/example.b:3.2.1")])[0].clone()
        }

        fn publish_stop_event(&self) {
            self.channels
                .app_publisher
                .publish(ApplicationEvent::StopRequested)
                .unwrap();
        }

        fn status_applying(&self, hash: Hash) -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: Applying as i32,
                last_remote_config_hash: hash.to_string().into_bytes(),
                error_message: "".to_string(),
            }
        }

        fn status_applied(&self, hash: Hash) -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: Applied as i32,
                last_remote_config_hash: hash.to_string().into_bytes(),
                error_message: "".to_string(),
            }
        }

        fn status_failed(&self, hash: Hash) -> RemoteConfigStatus {
            RemoteConfigStatus {
                status: Failed as i32,
                last_remote_config_hash: hash.to_string().into_bytes(),
                error_message: "".to_string(),
            }
        }

        /// Executes the provided function using the stored remote configuration. It panics if there is any error
        /// receiving the remote configuration or there is no remote configuration.
        fn assert_stored_remote_config<F: Fn(RemoteConfig)>(&self, f: F) {
            let config = self
                .dyn_config_store
                .values_repository
                .get_remote_config(&AgentID::AgentControl)
                .expect("Error getting the remote config for Agent control")
                .expect("No remote config found for Agent Control");
            f(config)
        }

        /// Checks that there is no remote configuration stored
        fn assert_no_persisted_remote_config(&self) {
            assert!(self.dyn_config_store.get_remote_config().unwrap().is_none())
        }

        /// Builds a remote configuration for AC corresponding to the provided string
        fn build_ac_remote_config(&self, s: &str) -> OpampRemoteConfig {
            let hash = Hash::new(s);
            OpampRemoteConfig::new(
                AgentID::AgentControl,
                hash,
                ConfigState::Applying,
                ConfigurationMap::new(HashMap::from([(
                    AGENT_CONFIG_PREFIX.to_string(),
                    s.to_string(),
                )])),
            )
        }

        /// Stores the remote config provided as string with the corresponding state
        fn store_remote_config(&self, s: &str, state: ConfigState) {
            let remote_config = RemoteConfig {
                config: YAMLConfig::try_from(s).unwrap(),
                hash: Hash::new(s),
                state,
            };
            self.dyn_config_store.store(&remote_config).unwrap();
        }

        /// Builds a dynamic configuration and the corresponding [StartedSubAgents] corresponding to the provided string representation
        fn build_current_config_and_sub_agents(
            &self,
            s: &str,
        ) -> (
            AgentControlDynamicConfig,
            StartedSubAgents<MockStartedSubAgent>,
        ) {
            let dyn_config = AgentControlDynamicConfig::try_from(s).unwrap();
            let started_sub_agents = self.build_started_subagents(&dyn_config);
            (dyn_config, started_sub_agents)
        }

        /// Builds a list of started sub-agents corresponding to the provided AgentControlDynamicConfig
        fn build_started_subagents(
            &self,
            dyn_config: &AgentControlDynamicConfig,
        ) -> StartedSubAgents<MockStartedSubAgent> {
            HashMap::from_iter(
                dyn_config
                    .agents
                    .keys()
                    .map(|id| (id.clone(), MockStartedSubAgent::new())),
            )
            .into()
        }

        /// Gets the corresponding entities from the provided config
        fn identities_from_agents_config(&self, s: &str) -> Vec<AgentIdentity> {
            let dyn_config = AgentControlDynamicConfig::try_from(s).unwrap();
            dyn_config
                .agents
                .into_iter()
                .map(|(id, cfg)| AgentIdentity {
                    id,
                    agent_type_id: cfg.agent_type,
                })
                .collect()
        }

        /// Helper to easily build identities
        fn identities(&self, values: Vec<(&str, &str)>) -> Vec<AgentIdentity> {
            values
                .into_iter()
                .map(|(id, at)| {
                    let agent_id = AgentID::try_from(id).unwrap();
                    let agent_type_id = AgentTypeID::try_from(at).unwrap();
                    AgentIdentity {
                        id: agent_id,
                        agent_type_id,
                    }
                })
                .collect()
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
            // remote config is valid by default, set it up for other expectations
            let remote_config_validator = TestRemoteConfigValidator { valid: true };
            let dynamic_config_validator = TestDynamicConfigValidator { valid: true };
            let sub_agent_builder = MockSubAgentBuilder::new();
            let (application_event_publisher, application_event_consumer) = pub_sub();
            let (opamp_publisher, opamp_consumer) = pub_sub();
            let mut agent_control_publisher = UnboundedBroadcast::default();
            let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
            let resource_cleaner = MockResourceCleaner::new();
            let version_updater = MockVersionUpdater::new();

            let ac_config = AgentControlConfig {
                health_check: AgentControlHealthCheckerConfig {
                    initial_delay: Duration::ZERO.into(),
                    ..Default::default()
                },
                ..Default::default()
            };

            let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();
            let agent_control = {
                AgentControl::new(
                    Some(started_client),
                    sub_agent_builder,
                    SystemTime::UNIX_EPOCH,
                    sa_dynamic_config_store.clone(),
                    agent_control_publisher,
                    application_event_consumer,
                    Some(opamp_consumer),
                    agent_control_internal_publisher,
                    agent_control_internal_consumer,
                    remote_config_validator,
                    dynamic_config_validator,
                    resource_cleaner,
                    version_updater,
                    NONE_MOCK_HEALTH_CHECKER_BUILDER,
                    ac_config,
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

        /// Sets if opamp configuration should be valid or not
        fn set_remote_config_valid(&mut self, valid: bool) {
            self.remote_config_validator = TestRemoteConfigValidator { valid }
        }

        /// Sets if dynamic configuration should be valid or not
        fn set_dynamic_config_valid(&mut self, valid: bool) {
            self.dynamic_config_validator = TestDynamicConfigValidator { valid }
        }

        /// Sets and stores (as local) the initial configuration corresponding to the provided string
        fn set_initial_config_local(&mut self, config: String) {
            let as_yaml = YAMLConfig::try_from(config).unwrap();
            // store as local
            self.sa_dynamic_config_store
                .values_repository
                .store_local(&AgentID::AgentControl, &as_yaml)
                .unwrap();
            // load the dynamic part
            let dyn_config = self.sa_dynamic_config_store.load().unwrap();
            // sets it up as initial config for AC
            self.initial_config = AgentControlConfig {
                dynamic: dyn_config,
                ..Default::default()
            };
        }

        /// Sets and stores (as remote) the initial configuration corresponding to the provided string
        fn set_initial_config_remote(&mut self, config: String, state: ConfigState) {
            let hash = Hash::new(&config);
            let as_yaml = YAMLConfig::try_from(config).unwrap();
            let remote_config = RemoteConfig {
                config: as_yaml,
                hash,
                state,
            };
            // store as local
            self.sa_dynamic_config_store.store(&remote_config).unwrap();
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
            self.set_sub_agent_build_success_with_expectations(identities, |started| {
                started.expect_stop().once().returning(|| Ok(()));
            });
        }

        /// Same as [Self::set_sub_agent_build_success] but sub-agents don't stop.
        fn set_sub_agent_build_success_no_stop(&mut self, identities: Vec<AgentIdentity>) {
            self.set_sub_agent_build_success_with_expectations(identities, |_| {});
        }

        /// Set expectations for each provided [AgentIdentity] to be built by the sub-agent builder.
        /// Each sub-agent build will expect to run and the expectations set in the the provided function
        /// are expected for started agents.
        fn set_sub_agent_build_success_with_expectations(
            &mut self,
            identities: Vec<AgentIdentity>,
            f: fn(&mut MockStartedSubAgent),
        ) {
            for identity in identities {
                // Clone f to ensure it's moved into the closure
                let ex = f;
                self.sub_agent_builder
                    .expect_build()
                    .once()
                    .with(predicate::eq(identity))
                    .returning(move |_| {
                        let mut not_started = MockNotStartedSubAgent::new();
                        not_started.expect_run().once().returning(move || {
                            let mut started = MockStartedSubAgent::new();
                            ex(&mut started);
                            started
                        });
                        Ok(not_started)
                    });
            }
        }

        /// Set expectations for each provided [AgentIdentity] to be built by the sub-agent builder.
        /// Each sub-agent build will fail.
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

    #[test]
    fn test_run_bootstrap_empty_agents() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_local("agents: {}\n".to_string());

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.publish_stop_event();
        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn test_run_bootstrap_multiple_agents_local() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_local(TestData::TWO_AGENTS_CONFIG.to_string());
        let identities = t.identities_from_agents_config(TestData::TWO_AGENTS_CONFIG);
        agent_control.set_sub_agent_build_success(identities);

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.publish_stop_event();
        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn test_run_bootstrap_agents_from_remote_config_applied_with_agents_ok() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_remote(
            TestData::SINGLE_AGENT_CONFIG.to_string(),
            ConfigState::Applied,
        );
        let identities = t.identities_from_agents_config(TestData::SINGLE_AGENT_CONFIG);
        agent_control.set_sub_agent_build_success(identities);

        agent_control.set_opamp_expectations(|client| {
            // It does not report RemoteConfig status because it hasn't changed from last time
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.publish_stop_event();
        assert!(agent_control.run().is_ok())
    }

    #[test]
    fn test_run_bootstrap_agents_from_remote_config_failed_with_agents_ok() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_remote(
            TestData::SINGLE_AGENT_CONFIG.to_string(),
            ConfigState::Failed {
                error_message: "some error".to_string(),
            },
        );
        let hash = Hash::new(TestData::SINGLE_AGENT_CONFIG);
        let identities = t.identities_from_agents_config(TestData::SINGLE_AGENT_CONFIG);
        agent_control.set_sub_agent_build_success(identities);

        agent_control.set_opamp_expectations(|client| {
            client
                .should_set_remote_config_status_matching_seq(vec![t.status_applied(hash.clone())]);
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.publish_stop_event();
        assert!(agent_control.run().is_ok());
        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, hash);
            assert!(config.is_applied())
        });
    }

    #[test]
    fn test_run_bootstrap_with_failing_agents_and_no_remote_config() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_local(TestData::TWO_AGENTS_CONFIG.to_string());

        agent_control.set_sub_agent_build_success(vec![t.two_agents_first_agent_identity()]);
        agent_control.set_sub_agent_build_fail(vec![t.two_agents_second_agent_identity()]);

        agent_control.set_opamp_expectations(|client| {
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        // AC will start and run but only with one agent
        t.publish_stop_event();
        assert!(agent_control.run().is_ok());
    }

    #[test]
    fn test_run_bootstrap_with_failing_agents_and_remote_config_applied() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_remote(
            TestData::TWO_AGENTS_CONFIG.to_string(),
            ConfigState::Applied,
        );
        let hash = Hash::new(TestData::TWO_AGENTS_CONFIG);
        agent_control.set_sub_agent_build_success(vec![t.two_agents_first_agent_identity()]);
        agent_control.set_sub_agent_build_fail(vec![t.two_agents_second_agent_identity()]);

        agent_control.set_opamp_expectations(|client| {
            client
                .should_set_remote_config_status_matching_seq(vec![t.status_failed(hash.clone())]);
            client.should_update_effective_config(1);
            client.should_stop(1);
        });

        t.publish_stop_event();
        assert!(agent_control.run().is_ok());
        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, hash);
            assert!(config.is_failed())
        });
    }

    #[test]
    /// Applying an invalid remote configuration publish the corresponding error. This configuration is invalid
    /// when received, the OpAMP layer already brought it as invalid. Eg: invalid signature.
    fn test_handle_remote_config_for_invalid_opamp_config() {
        let (t, mut agent_control) = TestAgentControl::setup();

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(TestData::SINGLE_AGENT_CONFIG);
        let mut opamp_remote_config = t.build_ac_remote_config("invalid");
        opamp_remote_config.state = ConfigState::Failed {
            error_message: "some error".to_string(),
        };

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_failed(opamp_remote_config.clone().hash),
            ]);
        });

        let result = agent_control.handle_remote_config(
            opamp_remote_config,
            &mut running_sub_agents,
            &current_dynamic_config,
        );

        assert_matches!(result, Err(AgentControlError::RemoteConfig(s)) => {
            assert!(s.to_string().contains("some error"))
        });
        t.assert_no_persisted_remote_config();
    }

    #[test]

    /// Checks that receiving an empty configuration removes the remote and applies the persisted local configuration.
    fn test_handle_remote_config_fallback_to_local_on_empty() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_updater();

        let local_config = TestData::SINGLE_AGENT_CONFIG;
        let previous_remote_config: &str = r#"
agents:
  remote-id:
    agent_type: "newrelic/remote.example:0.0.2"
        "#;
        agent_control.set_initial_config_local(local_config.to_string());
        t.store_remote_config(previous_remote_config, ConfigState::Applied);

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(previous_remote_config);

        let opamp_remote_config = t.build_ac_remote_config(""); // Empty means going back to local

        // Current agent should stop and be garbage collected
        running_sub_agents.agents().values_mut().for_each(|agent| {
            agent.should_stop();
        });
        agent_control.expect_resource_clean_in_sequence(
            t.identities_from_agents_config(previous_remote_config),
        );

        // Local agents should start
        let local_identities = t.identities_from_agents_config(local_config);
        agent_control.set_sub_agent_build_success_no_stop(local_identities.clone());

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_applied(opamp_remote_config.clone().hash),
            ]);
            client.should_update_effective_config(1);
        });

        let result = agent_control.handle_remote_config(
            opamp_remote_config,
            &mut running_sub_agents,
            &current_dynamic_config,
        );
        assert_matches!(result, Ok(dynamic_config) => {
            assert!(dynamic_config != current_dynamic_config);
            assert_eq!(dynamic_config.agents.len(), 1);
            assert_eq!(dynamic_config.agents.into_iter().next().unwrap().0, local_identities[0].id);
        });
        t.assert_no_persisted_remote_config();
    }

    #[test]
    /// Checks that the agent configuration does not apply if a version update fails.
    fn test_handle_remote_config_version_update_fails() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(TestData::SINGLE_AGENT_CONFIG);
        let remote_config: &str = r#"
agents:
  remote-id:
    agent_type: "newrelic/remote.example:0.0.2"
chart_version: 0.0.1 # Set for consistency but it is actually unused since we use a mock
        "#;
        let opamp_remote_config = t.build_ac_remote_config(remote_config);

        // Version update fails
        let dyn_config = AgentControlDynamicConfig::try_from(remote_config).unwrap();
        agent_control
            .version_updater
            .expect_update()
            .once()
            .with(predicate::eq(dyn_config))
            .returning(|_| Err(UpdaterError::UpdateFailed("failure".to_string())));

        running_sub_agents.agents().values_mut().for_each(|agent| {
            agent.expect_stop().never(); // current agents should not stop
        });

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_failed(opamp_remote_config.clone().hash),
            ]);
        });

        let result = agent_control.handle_remote_config(
            opamp_remote_config,
            &mut running_sub_agents,
            &current_dynamic_config,
        );
        assert_matches!(result, Err(AgentControlError::Updater(_)));
        t.assert_no_persisted_remote_config(); // When the updater fails the remote configuration is not persisted
    }

    #[test]
    /// Checks that the sub-agents are stopped, deleted and added as expected when multiple agents change.
    fn test_handle_remote_config_multiple_agents_change() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_updater();

        let current_config: &str = r#"
agents:
  id1:
    agent_type: "newrelic/remote.example.a:0.0.1"
  id2:
    agent_type: "newrelic/remote.example.b:0.0.2"
  id3:
    agent_type: "newrelic/remote.example.c:0.0.3"
        "#;

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(current_config);

        let remote_config: &str = r#"
agents:
  id1:
    agent_type: "newrelic/remote.example.a:0.0.1" # remains the same
  id2:
    agent_type: "newrelic/remote.example.b2:0.0.2" # the type changes
  id4:
    agent_type: "newrelic/remote.example.d:0.0.4" # new agent added
  # id3 is removed
        "#;
        let opamp_remote_config = t.build_ac_remote_config(remote_config);

        running_sub_agents
            .agents()
            .iter_mut()
            .for_each(|(id, agent)| {
                if &id.to_string() == "id1" {
                    // "id1" should remain the same
                    agent.expect_stop().never();
                } else {
                    agent.expect_stop().once().returning(|| Ok(())); // The rest of sub-agents should stop
                }
            });

        let identities: Vec<AgentIdentity> = t
            .identities_from_agents_config(remote_config)
            .into_iter()
            .filter(|identity| &identity.id.to_string() != "id1") // We should keep old "id1"
            .collect();
        agent_control.set_sub_agent_build_success_no_stop(identities);

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_applied(opamp_remote_config.clone().hash),
            ]);
            client.should_update_effective_config(1);
        });

        // Different sequences because the order is not important as all of them are cleaned as part of the same
        // remote config.
        agent_control.expect_resource_clean_in_sequence(
            t.identities(vec![("id2", "newrelic/remote.example.b:0.0.2")]), // type changed
        );
        agent_control.expect_resource_clean_in_sequence(
            t.identities(vec![("id3", "newrelic/remote.example.c:0.0.3")]), // removed
        );

        let result = agent_control.handle_remote_config(
            opamp_remote_config.clone(),
            &mut running_sub_agents,
            &current_dynamic_config,
        );

        assert_matches!(result, Ok(dynamic_config) => {
            assert_eq!(dynamic_config.agents.len(), 3);
            assert_eq!(
                dynamic_config.agents.keys().map(|k| k.to_string()).collect::<HashSet<String>>(),
                HashSet::from(["id1".to_string(), "id2".to_string(), "id4".to_string()]),
            );
        });
        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert!(config.state.is_applied());
        });
    }

    #[test]
    /// Checks the Agent Control behavior when one of the sub-agents fails to start.
    /// Agent Control tries to start as many agents as possible and persists the configuration
    /// (even if its not successful). The method `test_validate_apply_store_remote_config` returns an error
    /// in order to report the failure through OpAMP (RemoteConfigStatus)
    fn test_handle_remote_config_some_sub_agent_fail_to_start() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(TestData::SINGLE_AGENT_CONFIG);
        let remote_config: &str = r#"
agents:
  remote-id1:
    agent_type: "newrelic/remote.example.a:0.0.1"
  remote-id2:
    agent_type: "newrelic/remote.example.b:0.0.2"
  remote-id3:
    agent_type: "newrelic/remote.example.b:0.0.3"
        "#;
        let opamp_remote_config = t.build_ac_remote_config(remote_config);

        running_sub_agents.agents().values_mut().for_each(|agent| {
            agent.expect_stop().once().returning(|| Ok(())); // The current agent control should stop
        });

        let identities = t.identities_from_agents_config(remote_config);
        // Only the sub-agent "remote-id2" fails to start
        for identity in identities {
            agent_control
                .sub_agent_builder
                .expect_build()
                .once()
                .with(predicate::eq(identity.clone()))
                .returning(move |_| {
                    if &identity.id.to_string() == "remote-id2" {
                        Err(SubAgentBuilderError::UnsupportedK8sObject(
                            "some error".to_string(),
                        ))
                    } else {
                        let mut not_started = MockNotStartedSubAgent::new();
                        not_started.expect_run().returning(MockStartedSubAgent::new);
                        Ok(not_started)
                    }
                });
        }

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_failed(opamp_remote_config.clone().hash),
            ]);
            client.should_update_effective_config(1);
        });

        let result = agent_control.handle_remote_config(
            opamp_remote_config.clone(),
            &mut running_sub_agents,
            &current_dynamic_config,
        );

        assert_matches!(result, Err(AgentControlError::BuildingSubagents(_)));
        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert!(config.state.is_failed());
        });
    }

    #[rstest]
    #[case::invalid_remote_config(|ac: &mut TestAgentControl| { ac.set_remote_config_valid(false)})]
    #[case::invalid_dynamic_config(|ac: &mut TestAgentControl| { ac.set_dynamic_config_valid(false)})]
    /// Checks that an invalid OpAMP config prevents version update to take place.
    fn test_handle_remote_config_invalid_config_prevents_update(
        #[case] setup: impl Fn(&mut TestAgentControl),
    ) {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        setup(&mut agent_control);
        agent_control.version_updater.expect_update().never(); // version updater should not be called

        let (current_dynamic_config, mut running_sub_agents) =
            t.build_current_config_and_sub_agents(TestData::SINGLE_AGENT_CONFIG);
        let remote_config: &str = r#"
agents:
  remote-id1: # not actually used, we rely on a mock
    agent_type: "non-existent/invalid:0.0.1"
chart_version: 0.0.2 # not actually used, we rely on a mock
"#;
        let opamp_remote_config = t.build_ac_remote_config(remote_config);

        running_sub_agents.agents().values_mut().for_each(|agent| {
            agent.expect_stop().never(); // The current agent control should not stop
        });

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(opamp_remote_config.clone().hash),
                t.status_failed(opamp_remote_config.clone().hash),
            ]);
        });

        let result = agent_control.handle_remote_config(
            opamp_remote_config.clone(),
            &mut running_sub_agents,
            &current_dynamic_config,
        );

        assert_matches!(result, Err(AgentControlError::RemoteConfigValidator(_)));
        t.assert_no_persisted_remote_config();
    }

    #[test]
    // This test makes sure that after receiving an "OpAMPEvent::Connected" the AC reports the corresponding
    // broadcast event
    fn test_process_events_receive_opamp_connected() {
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

        t.publish_stop_event();
        assert!(event_processor.join().is_ok());
    }

    #[test]
    // This tests makes sure that after receiving an "OpAMPEvent::ConnectFailed" the AC reports the corresponding
    // broadcast event
    fn test_process_events_receive_opamp_connect_failed() {
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
        t.publish_stop_event();
        assert!(event_processor.join().is_ok())
    }

    // Health Checker events are correctly published
    #[test]
    fn test_process_events_health_checker() {
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

        // Leave some time for the health-checker to execute
        sleep(Duration::from_millis(250));

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
    fn test_process_events_stop_request() {
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
    fn test_process_events_remove_sub_agent() {
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

        agent_control.set_initial_config_local(TestData::SINGLE_AGENT_CONFIG.to_string());
        let dyn_config =
            AgentControlDynamicConfig::try_from(TestData::SINGLE_AGENT_CONFIG).unwrap();
        let agent_id = dyn_config.agents.keys().next().unwrap().clone();

        let remote_config = "agents: {}";
        let opamp_remote_config = t.build_ac_remote_config(remote_config);

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

        t.publish_stop_event();
        assert!(event_processor.join().is_ok());

        let expected = AgentControlEvent::SubAgentRemoved(agent_id);
        let ev = t.channels.broadcast_subscriber.as_ref().recv().unwrap();
        assert_eq!(expected, ev);

        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            let expected_yaml_config = serde_yaml::from_str(remote_config).unwrap();
            assert_eq!(config.config, expected_yaml_config);
            assert!(config.state.is_applied());
        });
    }

    #[test]
    /// Checks the Agent Control behavior when it receives a remote configuration
    fn test_run_receive_opamp_remote_config() {
        let (t, mut agent_control) = TestAgentControl::setup();
        agent_control.set_noop_resource_cleaner();
        agent_control.set_noop_updater();

        agent_control.set_initial_config_local(TestData::SINGLE_AGENT_CONFIG.to_string());
        let mut identities = t.identities_from_agents_config(TestData::SINGLE_AGENT_CONFIG);

        let remote_config: &str = r#"
agents:
  remote-id:
    agent_type: "newrelic/remote.example:0.0.42"
    "#;
        let hash = Hash::new(remote_config);
        identities.extend(t.identities_from_agents_config(remote_config));

        agent_control.set_sub_agent_build_success(identities); // For each agent in local and remote AC should build, start and stop

        agent_control.set_opamp_expectations(|client| {
            client.should_set_remote_config_status_matching_seq(vec![
                t.status_applying(hash.clone()),
                t.status_applied(hash.clone()),
            ]);
            client.should_update_effective_config(2); // one for initial config, one for remote config
            client.should_stop(1);
        });

        let running_agent_control = spawn(move || agent_control.run());

        // Publish remote config
        let opamp_remote_config = t.build_ac_remote_config(remote_config);
        t.channels
            .opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(
                opamp_remote_config.clone(),
            ))
            .unwrap();
        sleep(Duration::from_millis(500));

        t.publish_stop_event();
        assert!(running_agent_control.join().is_ok());

        t.assert_stored_remote_config(|config| {
            assert_eq!(config.hash, opamp_remote_config.hash);
            assert!(config.state.is_applied());
        });
    }
}
