use crate::agent_type::environment::Environment;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::opamp::remote_config_report::report_remote_config_status_error;
use crate::opamp::remote_config_report::{
    report_remote_config_status_applied, report_remote_config_status_applying,
};
use crate::sub_agent::config_validator::ConfigValidator;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentError};
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::event_handler::opamp::remote_config::store_remote_config_hash_and_values;
use crate::sub_agent::health::health_checker::log_and_report_unhealthy;
use crate::sub_agent::supervisor::{SupervisorBuilder, SupervisorStarter, SupervisorStopper};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::values::yaml_config_repository::YAMLConfigRepository;

use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{debug, error, warn};

use super::supervisor::SupervisorError;

pub(crate) type SubAgentCallbacks<C> = AgentCallbacks<C>;

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
    fn stop(self);
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError>;
}

/// SubAgentStopper is implementing the StartedSubAgent trait.
///
/// It stores the runtime JoinHandle and a SubAgentInternalEvent publisher.
/// It's stored in the super-agent's NotStartedSubAgents collection to be able to call
/// the exposed method Stop that will publish a StopRequested event to the runtime
/// and wait on the JoinHandle for the runtime to finish.
pub struct SubAgentStopper {
    agent_id: AgentID,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    runtime: JoinHandle<Result<(), SubAgentError>>,
}

/// SubAgent is implementing the NotStartedSubAgent trait so only the method run
/// can be called from the SuperAgent to start the runtime and receive a StartedSubAgent
/// that can be stopped
///
/// All its methods are internal and only called from the runtime method that spawns
/// a thread listening to events and acting on them.
pub struct SubAgent<C, CB, A, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<OpAMPClient = C> + Send + Sync + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    pub(super) agent_id: AgentID,
    pub(super) agent_cfg: SubAgentConfig,
    pub(super) maybe_opamp_client: Option<C>,
    pub(super) effective_agent_assembler: Arc<A>,
    pub(super) supervisor_builder: B,
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(super) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    pub(super) sub_agent_remote_config_hash_repository: Arc<HS>,
    pub(super) remote_values_repo: Arc<Y>,
    pub(super) config_validator: Arc<ConfigValidator>,
    pub(super) environment: Environment,

    // This is needed to ensure the generic type parameter CB is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_callbacks: PhantomData<CB>,
}

impl<C, CB, A, B, HS, Y> SubAgent<C, CB, A, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<OpAMPClient = C> + Send + Sync + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        effective_agent_assembler: Arc<A>,
        maybe_opamp_client: Option<C>,
        supervisor_builder: B,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        internal_pub_sub: (
            EventPublisher<SubAgentInternalEvent>,
            EventConsumer<SubAgentInternalEvent>,
        ),
        sub_agent_remote_config_hash_repository: Arc<HS>,
        remote_values_repo: Arc<Y>,
        config_validator: Arc<ConfigValidator>,
        environment: Environment,
    ) -> Self {
        Self {
            agent_id,
            agent_cfg,
            effective_agent_assembler,
            maybe_opamp_client,
            supervisor_builder,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher: internal_pub_sub.0,
            sub_agent_internal_consumer: internal_pub_sub.1,
            sub_agent_remote_config_hash_repository,
            remote_values_repo,
            config_validator,
            environment,

            _opamp_callbacks: PhantomData,
        }
    }

    pub fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        thread::spawn(move || {
            // Start the new supervisor if any, without hash as it's the first time
            let mut supervisor = self
                .generate_supervisor()
                .and_then(|s| self.start_supervisor(s))
                .ok();

            debug!(
                agent_id = %self.agent_id,
                "runtime started"
            );

            Option::as_ref(&self.maybe_opamp_client).map(|client| client.update_effective_config());

            // The below two lines are used to create a channel that never receives any message
            // if the sub_agent_opamp_consumer is None. Thus, we avoid erroring if there is no
            // publisher for OpAMP events and we attempt to receive them, as erroring while reading
            // from this channel will break the loop and prevent the reception of sub-agent
            // internal events if OpAMP is globally disabled in the super-agent config.
            let never_receive = EventConsumer::from(never());
            let opamp_receiver = self
                .sub_agent_opamp_consumer
                .as_ref()
                .unwrap_or(&never_receive);
            // TODO: We should separate the loop for OpAMP events and internal events into two
            // different loops, which currently is not straight forward due to sharing structures
            // that need to be moved into thread closures.
            loop {
                select! {
                    recv(opamp_receiver.as_ref()) -> opamp_event_res => {
                        match opamp_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_opamp_consumer", "channel closed");
                                break;
                            }

                            Ok(OpAMPEvent::RemoteConfigReceived(mut config)) => {
                                debug!(agent_id = self.agent_id.to_string(),
                                select_arm = "sub_agent_opamp_consumer",
                "remote config received");

                                // This branch only makes sense with a valid OpAMP client
                                let Some(opamp_client) = &self.maybe_opamp_client else {
                                    debug!(select_arm = "sub_agent_opamp_consumer", "got remote config without OpAMP being enabled");
                                    continue;
                                };

                                if let Err(e) = self.config_validator.validate(&self.agent_cfg.agent_type, &config) {
                                    error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error validating remote config");
                                    // This reporting might fail as well... what do we do?
                                    if let Err(e) = report_remote_config_status_error(opamp_client, &config.hash, e.to_string()) {
                                        error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error reporting remote config status");
                                    }
                                    continue;
                                }

                                if let Err(e) = report_remote_config_status_applying(opamp_client, &config.hash) {
                                    error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error reporting remote config status");
                                    continue;
                                }

                                // FIXME storing is a sensitive operation, what are the failure modes and what should we do when they happen?
                                if let Err(e) = store_remote_config_hash_and_values(&mut config, self.sub_agent_remote_config_hash_repository.as_ref(), self.remote_values_repo.as_ref()) {
                                    error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error storing remote config hash and values");
                                    // This reporting might fail as well... what do we do?
                                    if let Err(e) = report_remote_config_status_error(opamp_client, &config.hash, e.to_string()) {
                                        error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error reporting remote config status");
                                    }
                                    continue;
                                }

                                // If we reach this then the remote config was successfully applied
                                // Stop the current supervisor if any
                                stop_supervisor(&self.agent_id, supervisor);

                                supervisor = self.generate_supervisor()
                                    .and_then(|s| self.start_supervisor(s))
                                    .ok();
                            },
                            Ok(OpAMPEvent::Connected) | Ok(OpAMPEvent::ConnectFailed(_, _)) => {},
                        }
                    },
                    recv(&self.sub_agent_internal_consumer.as_ref()) -> sub_agent_internal_event_res => {
                        match sub_agent_internal_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_internal_consumer", "channel closed");
                                break;
                            }
                            Ok(SubAgentInternalEvent::StopRequested) => {
                                debug!(select_arm = "sub_agent_internal_consumer", "StopRequested");
                                stop_supervisor(&self.agent_id, supervisor);
                                break;
                            },
                            Ok(SubAgentInternalEvent::AgentHealthInfo(health))=>{
                                let _ = on_health(
                                    health,
                                    self.maybe_opamp_client.as_ref(),
                                    self.sub_agent_publisher.clone(),
                                    self.agent_id.clone(),
                                    self.agent_cfg.agent_type.clone(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing health message"));
                            }
                        }
                    }
                }
            }

            stop_opamp_client(self.maybe_opamp_client, &self.agent_id)
        })
    }

    fn generate_supervisor(&self) -> Result<B::SupervisorStarter, SupervisorError> {
        // Attempt to retrieve the hash
        let hash = self
            .sub_agent_remote_config_hash_repository
            .get(&self.agent_id)
            .inspect_err(|e| debug!(%self.agent_id, err = %e, "failed to get hash from repository"))
            .unwrap_or_default();

        if hash.is_none() {
            debug!(%self.agent_id, "no previous remote config found");
        }

        // Assemble the new agent
        let effective_agent_result = self.effective_agent_assembler.assemble_agent(
            &self.agent_id,
            &self.agent_cfg,
            &self.environment,
        );

        match effective_agent_result {
            Err(e) => {
                if let (Some(mut hash), Some(opamp_client)) = (hash, &self.maybe_opamp_client) {
                    if !hash.is_failed() {
                        hash.fail(e.to_string());
                        _ = self.sub_agent_remote_config_hash_repository.save(&self.agent_id, &hash).inspect_err(|e| debug!(%self.agent_id, err = %e, "failed to save hash to repository"));
                        // FIXME what do we do on failure above?
                    }
                    _ = report_remote_config_status_error(opamp_client, &hash, e.to_string())
                        .inspect_err(
                            |e| error!(%self.agent_id, %e, "error reporting remote config status"),
                        );
                }
                error!(agent_id=%self.agent_id, err = %e, "Error building the supervisor");
                Err(SupervisorError::BuildError(e.into()))
            }
            Ok(effective_agent) => {
                if let (Some(mut hash), Some(opamp_client)) = (hash, &self.maybe_opamp_client) {
                    if hash.is_applying() {
                        debug!(%self.agent_id, "applying remote config");
                        hash.apply();
                        _ = self.sub_agent_remote_config_hash_repository.save(&self.agent_id, &hash).inspect_err(|e| debug!(%self.agent_id, err = %e, "failed to save hash to repository")); // FIXME what do we do on failure?
                        _ = opamp_client.update_effective_config().inspect_err(
                            |e| error!(%self.agent_id, %e, "effective config update failed"),
                        ); // FIXME what do we do on failure?
                        _ = report_remote_config_status_applied(opamp_client, &hash).inspect_err(
                            |e| error!(%self.agent_id, %e, "error reporting remote config status"),
                        ); // FIXME what do we do on failure?
                    }
                    if let Some(err) = hash.error_message() {
                        warn!(%self.agent_id, err = %err, "remote config failed. Building with previous stored config");
                        _ = report_remote_config_status_error(opamp_client, &hash, err).inspect_err(|e| error!(%self.agent_id, %e, "error reporting remote config status"));
                        // FIXME what do we do on failure?
                    }
                }
                self.build_supervisor(effective_agent)
            }
        }
    }

    pub(crate) fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<B::SupervisorStarter, SupervisorError> {
        self.supervisor_builder
            .build_supervisor(effective_agent)
            .map_err(|err| {
                error!(agent_id=%self.agent_id, %err, "Error building the supervisor");
                err.into()
            })
    }

    pub(crate) fn start_supervisor(
        &self,
        not_started_supervisor: B::SupervisorStarter,
    ) -> Result<<B::SupervisorStarter as SupervisorStarter>::SupervisorStopper, SupervisorError>
    {
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
}

impl StartedSubAgent for SubAgentStopper {
    fn stop(self) {
        // Stop processing events
        let _ = self
            .sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)
            .inspect_err(|err| {
                error!(
                    agent_id = %self.agent_id,
                    %err,
                    "Error stopping event loop"
                )
            })
            .inspect(|_| {
                let _ = self.runtime.join().inspect_err(|_| {
                    error!(
                        agent_id = %self.agent_id,
                        "Error stopping event thread"
                    );
                });
            });
    }
}

pub fn stop_supervisor<S>(agent_id: &AgentID, maybe_started_supervisor: Option<S>)
where
    S: SupervisorStopper,
{
    if let Some(s) = maybe_started_supervisor {
        let _ = s.stop().inspect_err(|err| {
            error!(
                    agent_id = %agent_id,
                    %err,
                    "Error stopping k8s supervisor"
            );
        });
    }
}

impl<C, CB, A, B, HS, Y> NotStartedSubAgent for SubAgent<C, CB, A, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<OpAMPClient = C> + Send + Sync + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    type StartedSubAgent = SubAgentStopper;

    fn run(self) -> Self::StartedSubAgent {
        let agent_id = self.agent_id.clone();
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let runtime_handle = self.runtime();

        SubAgentStopper {
            agent_id,
            sub_agent_internal_publisher,
            runtime: runtime_handle,
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::hash_repository::repository::HashRepositoryError;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
    use crate::sub_agent::supervisor::test::{
        MockSupervisorBuilder, MockSupervisorStarter, MockSupervisorStopper,
    };
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use mockall::{mock, predicate};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};
    use std::collections::HashMap;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::traced_test;

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self);
        }
    }

    impl MockStartedSubAgent {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().return_const(());
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
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_config: &SubAgentConfig,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent,  SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _| {
                let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                not_started_sub_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent.expect_stop().times(1).return_const(());
                    started_agent
                });
                Ok(not_started_sub_agent)
            });
        }
    }

    type SubAgentForTesting = SubAgent<
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>,
        AgentCallbacks<MockEffectiveConfigLoaderMock>,
        MockEffectiveAgentAssemblerMock,
        MockSupervisorBuilder<MockSupervisorStarter>,
        MockHashRepositoryMock,
        MockYAMLConfigRepositoryMock,
    >;

    impl Default for SubAgentForTesting {
        fn default() -> Self {
            let agent_id = AgentID::new("some-agent-id").unwrap();
            let agent_cfg = SubAgentConfig {
                agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            };

            let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
            let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

            let mut sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
            sub_agent_remote_config_hash_repository
                .expect_get()
                .with(predicate::eq(agent_id.clone()))
                .return_const(Ok(None));
            let remote_values_repo = MockYAMLConfigRepositoryMock::default();

            let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
            let mut assembler = MockEffectiveAgentAssemblerMock::new();
            assembler.should_assemble_agent(
                &agent_id,
                &agent_cfg,
                &Environment::OnHost,
                effective_agent.clone(),
                1,
            );

            let mut supervisor_stopper = MockSupervisorStopper::new();
            supervisor_stopper
                .expect_stop()
                .times(0..=1) // at most once
                .return_once(|| Ok(()));

            let mut supervisor_starter = MockSupervisorStarter::new();
            supervisor_starter
                .expect_start()
                .times(0..=1) // at most once
                .with(predicate::always())
                .return_once(|_| Ok(supervisor_stopper));

            let mut supervisor_builder = MockSupervisorBuilder::new();
            supervisor_builder
                .expect_build_supervisor()
                .with(predicate::function(move |e: &EffectiveAgent| {
                    e == &effective_agent
                }))
                .return_once(|_| Ok(supervisor_starter));

            SubAgent::new(
                agent_id,
                agent_cfg,
                Arc::new(assembler),
                None,
                supervisor_builder,
                sub_agent_publisher,
                None,
                (sub_agent_internal_publisher, sub_agent_internal_consumer),
                Arc::new(sub_agent_remote_config_hash_repository),
                Arc::new(remote_values_repo),
                Arc::new(
                    ConfigValidator::try_new()
                        .expect("Failed to compile config validation regexes"),
                ),
                Environment::OnHost,
            )
        }
    }

    fn final_agent(agent_id: AgentID, agent_fqn: AgentTypeFQN) -> EffectiveAgent {
        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost::default()),
                    k8s: None,
                },
            },
        )
    }

    #[traced_test]
    #[test]
    fn test_run_and_stop() {
        let sub_agent = SubAgentForTesting::default();
        let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        started_agent.stop();

        assert!(!logs_contain("ERROR"));
    }

    #[traced_test]
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
        started_agent.stop();

        // This error is triggered since we try to send a StopRequested event on a closed channel
        assert!(logs_contain("Error stopping event loop"));
    }

    #[test]
    fn test_run_remote_config() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        let mut sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let mut remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
            2,
        );

        let mut supervisor_stopper = MockSupervisorStopper::new();
        supervisor_stopper
            .expect_stop()
            .once()
            .return_once(|| Ok(()));

        let mut supervisor_starter = MockSupervisorStarter::new();
        supervisor_starter
            .expect_start()
            .once()
            .with(predicate::always())
            .return_once(|_| Ok(supervisor_stopper));

        let mut supervisor_builder: MockSupervisorBuilder<MockSupervisorStarter> =
            MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(supervisor_starter));

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        sub_agent_remote_config_hash_repository.should_save_hash(&agent_id, &hash);
        remote_values_repo.should_store_remote(
            &agent_id,
            &YAMLConfig::new(HashMap::from([("some_item".into(), "some_value".into())])),
        );

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();

        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        });

        opamp_client
            .expect_update_effective_config()
            .once()
            .returning(|| Ok(()));

        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            Some(opamp_client),
            supervisor_builder,
            sub_agent_publisher,
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
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
        started_agent.stop();
    }

    // Tests for `generate_supervisor` function
    // Essentially, the function defines the behavior for a certain combination
    // of the following parameters:
    //
    // - The presence of an OpAMP client. Can be either `Some(opamp_client)` or `None`.
    // - The presence of a hash in the hash repository for the given agent_id: The call to `hash_repository.get(agent_id)?` done inside the function returns either `Some(Hash)` or `None`.
    // - The result of the agent assembly attempt. Can be either `Ok(EffectiveAgent)` or `Err(EffectiveAgentsAssemblerError)`.
    //
    // When the OpAMP client is `None` the function `hash_repository.get(agent_id)?` won't be called, there's no value to check for.
    // We are safe to discard those from the testing set and only look at the effective agent assemble result in this case.
    //
    // So, we cover all cases.

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_ok_eff_agent() {
        //  create a default subagent
        let mut sub_agent = SubAgentForTesting::default();

        // Modify expectations for this test
        // Expected calls on the hash repository
        let hash = Hash::new("some_hash".to_string());
        let mut applied_hash = hash.clone();
        applied_hash.apply();
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&sub_agent.agent_id, hash);
        hash_repository.should_save_hash(&sub_agent.agent_id, &applied_hash);

        sub_agent.sub_agent_remote_config_hash_repository = Arc::new(hash_repository);

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: "some_hash".as_bytes().to_vec(),
            status: Applied as i32,
            error_message: "".to_string(),
        });

        started_opamp_client.should_update_effective_config(1);
        sub_agent.maybe_opamp_client = Some(started_opamp_client);

        assert!(sub_agent.generate_supervisor().is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id) fails` must not be different from the `None` cases, but we test it anyway to detect if this invariant changes
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_err_hash_ok_eff_agent() {
        //  create a default subagent
        let mut sub_agent = SubAgentForTesting::default();

        // Modify expectations for this test
        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_return_error_on_get(
            &sub_agent.agent_id,
            HashRepositoryError::LoadError(String::from("random error loading")),
        );

        sub_agent.sub_agent_remote_config_hash_repository = Arc::new(hash_repository);

        // Expected calls on the opamp client
        sub_agent.maybe_opamp_client = Some(MockStartedOpAMPClientMock::new());

        assert!(sub_agent.generate_supervisor().is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_err_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash = Hash::new("some_hash".to_string());
        hash.fail("error assembling agents: `a random error happened!`".to_string());

        let expected_remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: hash.get().as_bytes().to_vec(),
            status: Failed as i32,
            error_message: hash.error_message().unwrap(),
        };

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_id, hash);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "a random error happened!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let mut opamp_client = MockStartedOpAMPClientMock::new();
        opamp_client.should_set_remote_config_status(expected_remote_config_status);

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            Some(opamp_client),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_err());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_ok_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_id);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let opamp_client = MockStartedOpAMPClientMock::new();

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            Some(opamp_client),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_err_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_id);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "a random error happened!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let opamp_client = MockStartedOpAMPClientMock::new();

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            Some(opamp_client),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_ok_eff_agent_no_opamp() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let hash = Hash::new("some_hash".to_string());
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_id, hash);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            None,
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_ok_eff_agent_no_opamp_no_hash() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_id);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            None,
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_err_eff_agent_no_opamp() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let hash = Hash::new("some_hash".to_string());
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_id, hash);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "random error!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            None,
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_err_eff_agent_no_opamp_no_hash() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_id);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "random error!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            None,
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        assert!(sub_agent.generate_supervisor().is_err());
    }

    /*
    // Tests for fn build_supervisor_or_default
    // They cannot be built as table tests as there are slight differences in
    // the actions of the scenarios.
    // Test cases:
    // -----------------------------------------------------------
    // Result(Hash) , Result(EffectiveAgent), Expected
    // -----------------------------------------------------------
    // Ok(Some)     , Ok                    , fn(effective agent)
    // Ok(Some)     , Err                   , fn(default)
    // Ok(None)     , Ok                    , fn(effective agent)
    // Ok(None)     , Err                   , fn(default)
    // Err          , Ok                    , fn(default)
    // Err          , Err                   , fn(effective agent)

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Ok(Some)     , Ok                    , fn(effective agent)
    #[test]
    fn test_build_supervisor_or_default_ok_some_ok() {
        // Mocks
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        let mut hash = Hash::new("some_hash".to_string());
        hash_repository.should_get_hash(&agent_id, hash.clone());

        // apply and save hash
        hash.apply();
        hash_repository.should_save_hash(&agent_id, &hash);
        // report remote config status
        started_opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Applied as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        });
        started_opamp_client.should_update_effective_config(1);

        // test build_supervisor_or_default
        let effective_agent_res = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = vec![agent_id];
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Ok(Some)     , Err                   , fn(default)
    #[test]
    fn test_build_supervisor_or_default_ok_err() {
        // Mocks
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        let mut hash = Hash::new("some_hash".to_string());
        hash_repository.should_get_hash(&agent_id, hash.clone());

        // apply and save hash
        hash.fail("error assembling agents: `some_error`".to_string());
        hash_repository.should_save_hash(&agent_id, &hash);
        // report remote config status
        started_opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: String::from("error assembling agents: `some_error`"),
        });

        // test build_supervisor_or_default
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = Vec::default();
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Ok(None)     , Err                    , fn(effective agent)
    #[test]
    fn test_build_supervisor_or_default_ok_none_ok() {
        // Mocks
        let started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        hash_repository.should_not_get_hash(&agent_id);

        // test build_supervisor_or_default
        let effective_agent_res = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = vec![agent_id];
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Ok(None)     , Err                    , fn(default)
    #[test]
    fn test_build_supervisor_or_default_ok_none_err() {
        // Mocks
        let started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        hash_repository.should_not_get_hash(&agent_id);

        // test build_supervisor_or_default
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = Vec::default();
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Err     , Ok                    , fn(effective agent)
    #[test]
    fn test_build_supervisor_or_default_err_ok() {
        // Mocks
        let started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        hash_repository.should_return_error_on_get(
            &agent_id,
            HashRepositoryError::LoadError("some_error".to_string()),
        );

        // test build_supervisor_or_default
        let effective_agent_res = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = vec![agent_id];
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    // Result(Hash) , Result(EffectiveAgent), Expected
    // Err     , Err                    , fn(effective agent)
    #[test]
    fn test_build_supervisor_or_default_err_err() {
        // Mocks
        let started_opamp_client = MockStartedOpAMPClientMock::new();
        let mut hash_repository = MockHashRepositoryMock::new();

        // hash repository should get hash by agentID
        let agent_id = AgentID::new("test-agent").unwrap();
        hash_repository.should_return_error_on_get(
            &agent_id,
            HashRepositoryError::LoadError("some_error".to_string()),
        );

        // test build_supervisor_or_default
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| Ok(vec![effective_agent.get_agent_id().clone()]),
        );

        let expected: Vec<AgentID> = Vec::default();
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    */
}
