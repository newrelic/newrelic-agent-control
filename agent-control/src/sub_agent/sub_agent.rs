use super::error::SubAgentStopError;
use super::health::health_checker::Health;
use crate::agent_control::defaults::default_capabilities;
use crate::agent_control::run::Environment;
use crate::agent_control::uptime_report::{UptimeReportConfig, UptimeReporter};
use crate::event::SubAgentEvent::SubAgentStarted;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::opamp::remote_config::RemoteConfig;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentError};
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::event_handler::on_version::on_version;
use crate::sub_agent::health::health_checker::log_and_report_unhealthy;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::remote_config_parser::RemoteConfigParser;
use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::utils::threads::spawn_named_thread;
use crate::values::yaml_config::YAMLConfig;
use crate::values::yaml_config_repository::{YAMLConfigRepository, load_remote_fallback_local};
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::StartedClient;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{debug, error, info, info_span, trace, warn};

/// NotStartedSubAgent exposes a run method that starts processing events and, if present, the supervisor.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;
    /// The run method (non-blocking) starts processing events and, if present, the supervisor.
    /// It returns a StartedSubAgent exposing .stop() to manage the running process.
    fn run(self) -> Self::StartedSubAgent;
}

/// The StartedSubAgent trait defines the interface for a supervisor that is already running.
///
/// Exposes information about the Sub Agent and a stop method that will stop the
/// supervised processes' execution and the loop processing the events.
pub trait StartedSubAgent {
    /// Stops all internal services owned by the SubAgent
    fn stop(self) -> Result<(), SubAgentStopError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_identity: &AgentIdentity,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError>;
}

/// SubAgentStopper is implementing the StartedSubAgent trait.
///
/// It stores the runtime JoinHandle and a SubAgentInternalEvent publisher.
/// It's stored in the agent-control's NotStartedSubAgents collection to be able to call
/// the exposed method Stop that will publish a StopRequested event to the runtime
/// and wait on the JoinHandle for the runtime to finish.
pub struct SubAgentStopper {
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    runtime: JoinHandle<Result<(), SubAgentError>>,
}

/// SubAgent is implementing the NotStartedSubAgent trait so only the method run
/// can be called from the AgentControl to start the runtime and receive a StartedSubAgent
/// that can be stopped
///
/// All its methods are internal and only called from the runtime method that spawns
/// a thread listening to events and acting on them.
pub struct SubAgent<C, SA, R, H, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    H: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    pub(super) identity: AgentIdentity,
    pub(super) maybe_opamp_client: Option<C>,
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(super) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    remote_config_parser: Arc<R>,
    supervisor_assembler: Arc<SA>,
    hash_repository: Arc<H>,
    yaml_config_repository: Arc<Y>,
    effective_agent_assembler: Arc<A>,
    environment: Environment,
}

impl<C, SA, R, H, Y, A> SubAgent<C, SA, R, H, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    H: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: AgentIdentity,
        maybe_opamp_client: Option<C>,
        supervisor_assembler: Arc<SA>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        internal_pub_sub: (
            EventPublisher<SubAgentInternalEvent>,
            EventConsumer<SubAgentInternalEvent>,
        ),
        remote_config_parser: Arc<R>,
        hash_repository: Arc<H>,
        values_repository: Arc<Y>,
        effective_agent_assembler: Arc<A>,
        environment: Environment,
    ) -> Self {
        Self {
            identity,
            maybe_opamp_client,
            supervisor_assembler,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher: internal_pub_sub.0,
            sub_agent_internal_consumer: internal_pub_sub.1,
            remote_config_parser,
            hash_repository,
            yaml_config_repository: values_repository,
            effective_agent_assembler,
            environment,
        }
    }

    pub fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        spawn_named_thread("Subagent runtime", move || {
            let span = info_span!("start_agent", id=%self.identity.id);
            let _span_guard = span.enter();

            let mut supervisor = self.assemble_and_start_supervisor();
            // Stores the current health state for logging purposes.
            let mut previous_health = None;

            debug!("runtime started");
            let _ = self.sub_agent_publisher
                .publish(SubAgentStarted(self.identity.clone(), SystemTime::now()))
                .inspect_err(|err| error!(error_msg = %err,"Cannot publish sub_agent_event::sub_agent_started"));

            self.maybe_opamp_client
                .as_ref()
                .map(|client| client.update_effective_config());

            // The below two lines are used to create a channel that never receives any message
            // if the sub_agent_opamp_consumer is None. Thus, we avoid erroring if there is no
            // publisher for OpAMP events, and we attempt to receive them, as erroring while reading
            // from this channel will break the loop and prevent the reception of sub-agent
            // internal events if OpAMP is globally disabled in the agent-control config.
            let never_receive = EventConsumer::from(never());
            let opamp_receiver = self
                .sub_agent_opamp_consumer
                .as_ref()
                .unwrap_or(&never_receive);
            // TODO: We should separate the loop for OpAMP events and internal events into two
            // different loops, which currently is not straight forward due to sharing structures
            // that need to be moved into thread closures.

            // Report uptime every 60 seconds
            let uptime_report_config = &UptimeReportConfig::default();
            let uptime_reporter = UptimeReporter::from(uptime_report_config);
            // If a uptime report is configured, we trace it for the first time here
            if uptime_report_config.enabled() {
                let _ = uptime_reporter.report();
            }

            drop(_span_guard);

            // Count the received remote configs during execution
            let mut remote_config_count = 0;
            loop {
                select! {
                    recv(opamp_receiver.as_ref()) -> opamp_event_res => {
                        let span = info_span!("process_fleet_event", id=%self.identity.id);
                        let _span_guard = span.enter();
                        match opamp_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_opamp_consumer", "Channel closed");
                                break;
                            }

                            Ok(OpAMPEvent::RemoteConfigReceived(config)) => {
                                debug!(
                                    select_arm = "sub_agent_opamp_consumer",
                                    "Remote config received"
                                );
                                // This branch only makes sense with a valid OpAMP client
                                let Some(opamp_client) = &self.maybe_opamp_client else {
                                    debug!("Got remote config without OpAMP being enabled");
                                    continue;
                                };
                                // Trace the occurrence of a remote config reception
                                remote_config_count += 1;
                                trace!(monotonic_counter.remote_configs_received = remote_config_count);

                                info!(hash=&config.hash.get(), "Applying remote config");
                                self.report_config_status(&config, opamp_client, OpampRemoteConfigStatus::Applying);

                                match self.remote_config_parser.parse(self.identity.clone(), &config) {
                                    Err(err) =>{
                                        warn!(hash=&config.hash.get(), "Remote configuration cannot be applied: {err}");
                                        self.report_config_status(&config, opamp_client, OpampRemoteConfigStatus::Error(err.to_string()));
                                        self.store_remote_config_hash(&config);
                                    },
                                    Ok(yaml_config) => {
                                        // TODO: we need to refactor the supervisor-assembler components in order to avoid persisting
                                        // and restarting the supervisor until the supervisor corresponding to the new configuration
                                        // is successfully.
                                        if let Err(err) = self.store_config_hash_and_values(&config, &yaml_config) {
                                            warn!(hash=&config.hash.get(), "Persisting remote configuration failed: {err}");
                                            self.report_config_status(&config, opamp_client, OpampRemoteConfigStatus::Error(err.to_string()));
                                        } else {
                                            // We need to restart the supervisor after we receive a new config
                                            // as we don't have hot-reloading handling implemented yet
                                            stop_supervisor(supervisor);
                                            supervisor = self.assemble_and_start_supervisor();
                                        }
                                    }
                                }
                            },
                            Ok(OpAMPEvent::Connected) | Ok(OpAMPEvent::ConnectFailed(_, _)) => {},
                        }
                    },
                    recv(&self.sub_agent_internal_consumer.as_ref()) -> sub_agent_internal_event_res => {
                        let span = info_span!("process_event", id=%self.identity.id);
                        let _span_guard = span.enter();
                        match sub_agent_internal_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_internal_consumer", "Channel closed");
                                break;
                            }
                            Ok(SubAgentInternalEvent::StopRequested) => {
                                debug!(select_arm = "sub_agent_internal_consumer", "StopRequested");
                                stop_supervisor(supervisor);
                                break;
                            },
                            Ok(SubAgentInternalEvent::AgentHealthInfo(health))=>{
                                debug!(select_arm = "sub_agent_internal_consumer", ?health, "AgentHealthInfo");

                                let health_state = Health::from(health.clone());
                                if !is_health_state_equal_to_previous_state(&previous_health, &health_state) {
                                    log_health_info(&health_state);
                                }
                                previous_health = Some(health_state);
                                let _ = on_health(
                                    health,
                                    self.maybe_opamp_client.as_ref(),
                                    self.sub_agent_publisher.clone(),
                                    self.identity.clone(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "Processing health message"));
                            }
                            Ok(SubAgentInternalEvent::AgentVersionInfo(agent_data)) => {
                                 let _ = on_version(
                                    agent_data,
                                    self.maybe_opamp_client.as_ref(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing version message"));
                            }
                        }
                    }
                    recv(uptime_reporter.receiver()) -> _tick => { let _ = uptime_reporter.report(); },
                }
            }

            stop_opamp_client(self.maybe_opamp_client, &self.identity.id)
        })
    }

    pub(crate) fn start_supervisor(
        &self,
        not_started_supervisor: SA::SupervisorStarter,
    ) -> Result<
        <SA::SupervisorStarter as SupervisorStarter>::SupervisorStopper,
        SupervisorStarterError,
    > {
        not_started_supervisor
            .start(self.sub_agent_internal_publisher.clone())
            .inspect_err(|err| {
                log_and_report_unhealthy(
                    &self.sub_agent_internal_publisher,
                    err,
                    "starting the resources supervisor failed",
                    SystemTime::now(),
                )
            })
    }

    fn assemble_and_start_supervisor(
        &self,
    ) -> Option<<SA::SupervisorStarter as SupervisorStarter>::SupervisorStopper> {
        let hash = self
            .hash_repository
            .get(&self.identity.id)
            .inspect_err(|e| debug!(err = %e, "failed to get hash from repository"))
            .unwrap_or_default();

        let effective_agent = self
            .effective_agent()
            .inspect_err(|e| {
                warn!("Cannot assemble effective agent: {}", e);
                if let (Some(mut hash), Some(opamp_client)) =
                    (hash.clone(), &self.maybe_opamp_client)
                {
                    if !hash.is_failed() {
                        hash.fail(e.to_string());
                        _ = self
                            .hash_repository
                            .save(&self.identity.id, &hash)
                            .inspect_err(|e| error!(err = %e, "failed to save hash to repository"));
                    }
                    _ = OpampRemoteConfigStatus::Error(e.to_string())
                        .report(opamp_client, &hash)
                        .inspect_err(|e| error!( %e, "error reporting remote config status"));
                }
            })
            .ok()?;

        if let (Some(mut hash), Some(opamp_client)) = (hash.clone(), &self.maybe_opamp_client) {
            if hash.is_applying() {
                debug!("applying remote config");
                hash.apply();
                _ = self
                    .hash_repository
                    .save(&self.identity.id, &hash)
                    .inspect_err(|e| error!( err = %e, "failed to save hash to repository"));
                _ = opamp_client
                    .update_effective_config()
                    .inspect_err(|e| error!( %e, "effective config update failed"));
                _ = OpampRemoteConfigStatus::Applied
                    .report(opamp_client, &hash)
                    .inspect_err(|e| error!( %e, "error reporting remote config status"));
            }
            if let Some(err) = hash.error_message() {
                warn!( err = %err, "remote config failed. Building with previous stored config");
                _ = OpampRemoteConfigStatus::Error(err)
                    .report(opamp_client, &hash)
                    .inspect_err(|e| error!( %e, "error reporting remote config status"));
            }
        }

        let stopped_supervisor = self
            .supervisor_assembler
            .assemble_supervisor(
                &self.maybe_opamp_client,
                self.identity.clone(),
                effective_agent,
            )
            .inspect_err(|e| warn!("Cannot assemble supervisor: {}", e))
            .ok();

        stopped_supervisor
            .map(|s| self.start_supervisor(s))
            .and_then(|s| s.ok())
    }

    fn effective_agent(&self) -> Result<EffectiveAgent, SubAgentError> {
        // Load the configuration
        let Some(yaml_config) = load_remote_fallback_local(
            self.yaml_config_repository.as_ref(),
            &self.identity.id,
            &default_capabilities(),
        )?
        else {
            debug!("There is no configuration for this agent");
            return Err(SubAgentError::NoConfiguration);
        };

        // Assemble the new agent
        Ok(self.effective_agent_assembler.assemble_agent(
            &self.identity,
            yaml_config,
            &self.environment,
        )?)
    }

    fn store_remote_config_hash(&self, config: &RemoteConfig) {
        let _ = self
            .hash_repository
            .save(&self.identity.id, &config.hash)
            .inspect_err(|err| {
                warn!(
                    hash = config.hash.get(),
                    "Could not save the hash repository: {err}"
                );
            });
    }

    fn store_config_hash_and_values(
        &self,
        config: &RemoteConfig,
        yaml_config: &Option<YAMLConfig>,
    ) -> Result<(), SubAgentError> {
        // Store remote config hash
        self.hash_repository.save(&self.identity.id, &config.hash)?;
        // Store remote config values
        match yaml_config {
            Some(yaml_config) => self
                .yaml_config_repository
                .store_remote(&self.identity.id, yaml_config),
            None => {
                debug!("Empty config received, remove remote configuration to fall-back to local");
                self.yaml_config_repository.delete_remote(&self.identity.id)
            }
        }?;
        Ok(())
    }

    fn report_config_status(
        &self,
        config: &RemoteConfig,
        opamp_client: &C,
        remote_config_status: OpampRemoteConfigStatus,
    ) {
        let _ = remote_config_status
            .report(opamp_client, &config.hash)
            .inspect_err(|e| {
                warn!("Reporting OpAMP configuration status failed: {e}");
            });
    }
}

fn is_health_state_equal_to_previous_state(
    previous_state: &Option<Health>,
    current_state: &Health,
) -> bool {
    match (previous_state, current_state) {
        (Some(Health::Healthy(_)), Health::Healthy(_)) => true,
        (Some(prev), current) => prev == current,
        _ => false,
    }
}

fn log_health_info(health: &Health) {
    match health {
        // From unhealthy (or initial) to healthy
        Health::Healthy(_) => {
            info!("Agent is healthy");
        }
        // Every time health is unhealthy
        Health::Unhealthy(unhealthy) => {
            warn!(
                status = unhealthy.status(),
                last_error = unhealthy.last_error(),
                "Agent is unhealthy"
            );
        }
    }
}

impl StartedSubAgent for SubAgentStopper {
    fn stop(self) -> Result<(), SubAgentStopError> {
        // Stop processing events
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;
        // Wait for the sub agent thread to finish
        let runtime_join_result = self.runtime.join().map_err(|_| {
            // Error when the 'runtime thread' panics.
            SubAgentStopError::SubAgentJoinHandle(
                "The sub agent thread failed unexpectedly".to_string(),
            )
        })?;
        Ok(runtime_join_result?)
    }
}

pub fn stop_supervisor<S>(maybe_started_supervisor: Option<S>)
where
    S: SupervisorStopper,
{
    if let Some(s) = maybe_started_supervisor {
        let _ = s.stop().inspect_err(|err| {
            error!(%err,"Error stopping supervisor");
        });
    }
}

impl<C, SA, R, H, Y, A> NotStartedSubAgent for SubAgent<C, SA, R, H, Y, A>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    H: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    type StartedSubAgent = SubAgentStopper;

    fn run(self) -> Self::StartedSubAgent {
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let runtime_handle = self.runtime();

        SubAgentStopper {
            sub_agent_internal_publisher,
            runtime: runtime_handle,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::onhost::OnHost;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::hash_repository::repository::tests::MockHashRepository;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssembler;
    use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
    use crate::sub_agent::remote_config_parser::tests::MockRemoteConfigParser;
    use crate::sub_agent::supervisor::assembler::tests::MockSupervisorAssembler;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepository;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};
    use rstest::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::traced_test;

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self) -> Result<(), SubAgentStopError>;
        }
    }

    impl MockStartedSubAgent {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().returning(|| Ok(()));
        }
    }

    mock! {
        pub NotStartedSubAgent {}

        impl NotStartedSubAgent for NotStartedSubAgent {
            type StartedSubAgent = MockStartedSubAgent;

            fn run(self) -> <Self as NotStartedSubAgent>::StartedSubAgent;
        }
    }

    impl MockNotStartedSubAgent {
        pub fn should_run(&mut self, started_sub_agent: MockStartedSubAgent) {
            self.expect_run()
                .once()
                .return_once(move || started_sub_agent);
        }
    }

    mock! {
        pub SubAgentBuilder {}

        impl SubAgentBuilder for SubAgentBuilder {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_identity: &AgentIdentity,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent,  SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilder {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _| {
                let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                not_started_sub_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent.expect_stop().times(1).returning(|| Ok(()));
                    started_agent
                });
                Ok(not_started_sub_agent)
            });
        }
    }

    type SubAgentForTesting = SubAgent<
        MockStartedOpAMPClient,
        MockSupervisorAssembler<MockSupervisorStarter>,
        MockRemoteConfigParser,
        MockHashRepository,
        MockYAMLConfigRepository,
        MockEffectiveAgentAssembler,
    >;

    impl Default for SubAgentForTesting {
        fn default() -> Self {
            let agent_identity = AgentIdentity::from((
                AgentID::new("some-agent-id").unwrap(),
                AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            ));

            let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
            let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

            let mut hash_repository = MockHashRepository::default();
            hash_repository
                .expect_get()
                .with(predicate::eq(agent_identity.id.clone()))
                .return_const(Ok(None));

            let yaml_repository = MockYAMLConfigRepository::new();

            let remote_config_parser = MockRemoteConfigParser::new();

            let mut started_supervisor = MockSupervisorStopper::new();
            started_supervisor.should_stop();

            let mut stopped_supervisor = MockSupervisorStarter::new();
            stopped_supervisor.should_start(started_supervisor);

            let effective_agents_assembler = MockEffectiveAgentAssembler::new();
            let effective_agent = EffectiveAgent::new(
                agent_identity.clone(),
                Runtime {
                    deployment: Deployment::default(),
                },
            );

            let mut supervisor_assembler = MockSupervisorAssembler::new();
            supervisor_assembler.should_assemble::<MockStartedOpAMPClient>(
                stopped_supervisor,
                agent_identity.clone(),
                effective_agent,
            );

            SubAgent::new(
                agent_identity,
                None,
                Arc::new(supervisor_assembler),
                sub_agent_publisher,
                None,
                (sub_agent_internal_publisher, sub_agent_internal_consumer),
                Arc::new(remote_config_parser),
                Arc::new(hash_repository),
                Arc::new(yaml_repository),
                Arc::new(effective_agents_assembler),
                Environment::OnHost,
            )
        }
    }

    #[traced_test]
    #[test]
    fn test_run_and_stop() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash_repository = MockHashRepository::default();
        hash_repository
            .expect_get()
            .with(predicate::eq(agent_identity.id.clone()))
            .return_const(Ok(None));

        let mut yaml_repository = MockYAMLConfigRepository::new();
        yaml_repository
            .expect_load_remote()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::eq(default_capabilities()),
            )
            .return_once(|_, _| Ok(Some(YAMLConfig::default())));

        let remote_config_parser = MockRemoteConfigParser::new();

        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        let mut effective_agents_assembler = MockEffectiveAgentAssembler::new();
        let effective_agent = EffectiveAgent::new(
            agent_identity.clone(),
            Runtime {
                deployment: Deployment::default(),
            },
        );
        effective_agents_assembler.should_assemble_agent(
            &agent_identity,
            &YAMLConfig::default(),
            &Environment::OnHost,
            effective_agent.clone(),
            1,
        );

        let mut supervisor_assembler = MockSupervisorAssembler::new();
        supervisor_assembler.should_assemble::<MockStartedOpAMPClient>(
            stopped_supervisor,
            agent_identity.clone(),
            effective_agent,
        );

        let sub_agent: SubAgent<
            MockStartedOpAMPClient,
            MockSupervisorAssembler<MockSupervisorStarter>,
            MockRemoteConfigParser,
            MockHashRepository,
            MockYAMLConfigRepository,
            MockEffectiveAgentAssembler,
        > = SubAgent::new(
            agent_identity,
            None,
            Arc::new(supervisor_assembler),
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(remote_config_parser),
            Arc::new(hash_repository),
            Arc::new(yaml_repository),
            Arc::new(effective_agents_assembler),
            Environment::OnHost,
        );

        let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        started_agent.stop().unwrap();

        assert!(!logs_contain("ERROR"));
    }

    #[test]
    fn test_run_and_fail_stop() {
        let mut sub_agent = SubAgentForTesting::default();
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        sub_agent.sub_agent_internal_publisher = sub_agent_internal_publisher.clone();
        sub_agent.sub_agent_internal_consumer = sub_agent_internal_consumer;

        // We send a message to end the runtime
        let publishing_result =
            sub_agent_internal_publisher.publish(SubAgentInternalEvent::StopRequested);
        assert!(publishing_result.is_ok());

        let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        let result = started_agent.stop();
        assert_matches!(result, Err(SubAgentStopError::SubAgentEventLoop(_)));
    }

    #[test]
    fn test_run_remote_config() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        // Event's config
        let hash = Hash::new(String::from("some-hash"));
        let mut applied_hash = hash.clone();
        applied_hash.apply();
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        let remote_config =
            RemoteConfig::new(agent_identity.id.clone(), hash.clone(), Some(config_map));

        let mut opamp_client = MockStartedOpAMPClient::new();
        opamp_client
            .expect_update_effective_config()
            .times(1)
            .returning(|| Ok(()));
        // Applying + Applied
        opamp_client.should_set_any_remote_config_status(1);

        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        // Assemble once on start
        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        let mut supervisor_assembler = MockSupervisorAssembler::new();

        let mut effective_agents_assembler = MockEffectiveAgentAssembler::new();
        let effective_agent = EffectiveAgent::new(
            agent_identity.clone(),
            Runtime {
                deployment: Deployment::default(),
            },
        );
        effective_agents_assembler.should_assemble_agent(
            &agent_identity,
            &YAMLConfig::default(),
            &Environment::OnHost,
            effective_agent.clone(),
            2,
        );

        supervisor_assembler.should_assemble::<MockStartedOpAMPClient>(
            stopped_supervisor,
            agent_identity.clone(),
            effective_agent.clone(),
        );

        let hash = Hash::new("some-hash".into());
        let yaml_config: YAMLConfig = serde_yaml::from_str("some_item: some_value").unwrap();

        let mut hash_repository = MockHashRepository::new();
        hash_repository
            .expect_get()
            .with(predicate::eq(agent_identity.id.clone()))
            .return_const(Ok(Some(applied_hash)));
        hash_repository.should_save_hash(&agent_identity.id, &hash);
        let mut yaml_repository = MockYAMLConfigRepository::new();
        yaml_repository
            .expect_load_remote()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::eq(default_capabilities()),
            )
            .return_const(Ok(Some(YAMLConfig::default())));
        yaml_repository.should_store_remote(&agent_identity.id, &yaml_config);

        // Receive a remote config
        let mut remote_config_parser = MockRemoteConfigParser::new();
        remote_config_parser.should_parse(
            agent_identity.clone(),
            remote_config.clone(),
            Some(yaml_config),
        );

        // Assemble again on config received
        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        supervisor_assembler.should_assemble::<MockStartedOpAMPClient>(
            stopped_supervisor,
            agent_identity.clone(),
            effective_agent,
        );

        let sub_agent = SubAgent::new(
            agent_identity,
            Some(opamp_client),
            Arc::new(supervisor_assembler),
            sub_agent_publisher,
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(remote_config_parser),
            Arc::new(hash_repository),
            Arc::new(yaml_repository),
            Arc::new(effective_agents_assembler),
            Environment::OnHost,
        );

        //start the runtime
        let started_agent = sub_agent.run();

        // publish event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(20));

        // stop the runtime
        started_agent.stop().unwrap();
    }

    #[rstest]
    #[case::healthy_states_same_status(Some(healthy("status")), healthy("status"))]
    #[case::healthy_states_different_status(Some(healthy("status a")), healthy("status b"))]
    #[case::unhealthy_states_with_same_content(
        Some(unhealthy("status", "error")),
        unhealthy("status", "error")
    )]
    fn test_health_state_is_equal_to_previous_state(
        #[case] previous_state: Option<Health>,
        #[case] current_state: Health,
    ) {
        assert!(is_health_state_equal_to_previous_state(
            &previous_state,
            &current_state
        ));
    }

    #[rstest]
    #[case::first_state_is_healthy(None, healthy("status"))]
    #[case::first_state_is_unhealthy(None, unhealthy("status", "error"))]
    #[case::healthy_and_unhealthy(Some(healthy("status")), unhealthy("status", "error"))]
    #[case::unhealthy_and_healthy(Some(unhealthy("status", "error")), healthy("status"))]
    #[case::two_unhealthy_states_with_different_status(
        Some(unhealthy("status a", "error")),
        unhealthy("status b", "error")
    )]
    #[case::two_unhealthy_states_with_different_errors(
        Some(unhealthy("status", "error a")),
        unhealthy("status", "error b")
    )]
    fn test_health_state_is_different_to_previous_state(
        #[case] previous_state: Option<Health>,
        #[case] current_state: Health,
    ) {
        assert!(!is_health_state_equal_to_previous_state(
            &previous_state,
            &current_state
        ));
    }

    #[fixture]
    fn agent_identity() -> AgentIdentity {
        AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ))
    }

    #[fixture]
    fn final_agent(agent_identity: AgentIdentity) -> EffectiveAgent {
        EffectiveAgent::new(
            agent_identity,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost::default()),
                    k8s: None,
                },
            },
        )
    }

    fn healthy(status: &str) -> Health {
        Health::Healthy(Healthy::new(status.to_string()))
    }

    fn unhealthy(status: &str, error: &str) -> Health {
        Health::Unhealthy(Unhealthy::new(status.to_string(), error.to_string()))
    }
}
