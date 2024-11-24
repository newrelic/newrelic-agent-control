use crate::agent_type::environment::Environment;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::opamp::remote_config_report::report_remote_config_status_applied;
use crate::opamp::remote_config_report::report_remote_config_status_error;
use crate::sub_agent::config_validator::ConfigValidator;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentError};
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::event_handler::opamp::remote_config::remote_config;
use crate::sub_agent::health::health_checker::log_and_report_unhealthy;
use crate::sub_agent::supervisor::{SupervisorBuilder, SupervisorStarter, SupervisorStopper};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::values::yaml_config_repository::YAMLConfigRepository;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::{Client, StartedClient};
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{debug, error, warn};

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
            // Build a new supervisor from the persisted values
            let maybe_not_started_supervisor = self.build_supervisor_from_persisted_values();
            // Start the new supervisor if any
            let mut supervisor = self.start_supervisor(maybe_not_started_supervisor);

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
                //TODO we are not actually testing that on a particular event we are calling the correct functions
                //TODO AgentHealthInfo and RemoteConfigReceived are not tested
                select! {
                    recv(opamp_receiver.as_ref()) -> opamp_event_res => {
                        match opamp_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_opamp_consumer", "channel closed");
                                break;
                            }

                            Ok(OpAMPEvent::RemoteConfigReceived(config)) => {
                                debug!(agent_id = self.agent_id.to_string(),
                                select_arm = "sub_agent_opamp_consumer",
                "remote config received");
                                if let Err(e) = remote_config(
                                    config,
                                    self.maybe_opamp_client.as_ref(),
                                    self.config_validator.as_ref(),
                                    self.remote_values_repo.as_ref(),
                                    self.sub_agent_remote_config_hash_repository.as_ref(),
                                    self.agent_cfg.agent_type.clone(),
                                ){
                                     error!(error = %e, select_arm = "sub_agent_opamp_consumer", "error processing remote config")
                                }

                                // Stop the current supervisor if any
                                stop_supervisor(&self.agent_id, supervisor);
                                // Build a new supervisor from the persisted values
                                let maybe_not_started_supervisor = self.build_supervisor_from_persisted_values();
                                // Start the new supervisor if any
                                supervisor = self.start_supervisor(maybe_not_started_supervisor);
                            }
                            _ => {}}
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

    pub(crate) fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    ) -> Option<B::SupervisorStarter> {
        self.supervisor_builder
            .build_supervisor(effective_agent_result, &self.maybe_opamp_client)
            .inspect_err(
                |err| error!(agent_id=%self.agent_id, %err, "Error building the supervisor"),
            )
            .unwrap_or_default()
    }

    pub(crate) fn start_supervisor(
        &self,
        maybe_not_started_supervisor: Option<B::SupervisorStarter>,
    ) -> Option<<B::SupervisorStarter as SupervisorStarter>::SupervisorStopper> {
        maybe_not_started_supervisor
            .map(|s| s.start(self.sub_agent_internal_publisher.clone()))
            .transpose()
            .inspect_err(|err| {
                log_and_report_unhealthy(
                    &self.sub_agent_internal_publisher,
                    err,
                    "starting the resources supervisor failed",
                    SystemTime::now(),
                )
            })
            .unwrap_or(None)
    }

    pub fn build_supervisor_from_persisted_values(&self) -> Option<B::SupervisorStarter> {
        let effective_agent_result = self.assemble_agent();
        self.build_supervisor(effective_agent_result)
    }

    pub fn assemble_agent(&self) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        self.effective_agent_assembler.assemble_agent(
            &self.agent_id,
            &self.agent_cfg,
            &self.environment,
        )
    }
}

pub(crate) fn build_supervisor_or_default<HR, O, T, F, C>(
    agent_id: &AgentID,
    hash_repository: &Arc<HR>,
    maybe_opamp_client: &Option<O::Client>,
    effective_agent_res: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    supervisor_builder_fn: F,
) -> Result<T, SubAgentBuilderError>
where
    HR: HashRepository,
    C: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<C>>,
    T: Default,
    F: FnOnce(EffectiveAgent) -> Result<T, SubAgentBuilderError>,
{
    // A sub-agent's supervisor can be started without a valid effective agent when an OpAMP
    // client is available. This is useful when the agent is in a failed state and the OpAMP
    // client is the only way to fix the configuration via remote configs.
    if let Some(opamp_client) = maybe_opamp_client {
        // // Invalid/corrupted hash file should not crash the sub agent
        let hash = hash_repository.get(agent_id).unwrap_or_else(|err| {
            error!(%agent_id, %err, "failed to get hash from repository");
            None
        });

        match (hash, effective_agent_res) {
            (Some(mut hash), Ok(effective_agent)) => {
                if hash.is_applying() {
                    debug!(%agent_id, "applying remote config");
                    hash.apply();
                    hash_repository.save(agent_id, &hash)?;
                    let _ = opamp_client.update_effective_config().inspect_err(|err| {
                        error!(%agent_id, %err, "effective config update failed");
                    });
                    report_remote_config_status_applied(opamp_client, &hash)?;
                }

                if let Some(err_msg) = hash.error_message() {
                    warn!(%agent_id, err = %err_msg, "remote config failed. Building with previous stored config");
                    report_remote_config_status_error(opamp_client, &hash, err_msg)?;
                }

                supervisor_builder_fn(effective_agent)
            }
            (Some(mut hash), Err(err)) => {
                if !hash.is_failed() {
                    hash.fail(err.to_string());
                    hash_repository.save(agent_id, &hash)?;
                }

                report_remote_config_status_error(opamp_client, &hash, err.to_string())?;
                error!(%agent_id, %err, "failed to assemble agent from remote config");
                Ok(Default::default())
            }
            (None, Err(err)) => {
                debug!(%agent_id, "no previous remote config found");
                warn!(%agent_id, %err, "no previous config found. Failed to assemble agent from local or remote config");
                Ok(Default::default())
            }
            (None, Ok(effective_agent)) => {
                debug!(%agent_id, "no previous remote config found");
                supervisor_builder_fn(effective_agent)
            }
        }
    } else {
        supervisor_builder_fn(effective_agent_res?)
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
    use crate::opamp::client_builder::test::MockOpAMPClientBuilderMock;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::hash_repository::repository::HashRepositoryError;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::supervisor::test::{MockSupervisorBuilder, MockSupervisorStarter};
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use mockall::{mock, predicate, Sequence};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying};
    use std::collections::HashMap;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::internal::logs_with_scope_contain;
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

    #[traced_test]
    #[test]
    fn test_run_and_stop() {
        let sub_agent = create_sub_agent_with_mocks();
        let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        started_agent.stop();

        assert!(!logs_contain("ERROR"));
    }

    #[traced_test]
    #[test]
    fn test_run_and_fail_stop() {
        let mut sub_agent = create_sub_agent_with_mocks();
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

        let mut supervisor_builder: MockSupervisorBuilder<MockSupervisorStarter> =
            MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(
                predicate::function(move |e: &Result<EffectiveAgent, _>| {
                    e.as_ref().is_ok_and(|x| *x == effective_agent)
                }),
                predicate::always(),
            )
            .returning(|_, _| Ok(None));

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

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::sub_agent",
            "remote config received",
        ));
    }

    type SubAgentForTesting = SubAgent<
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>,
        AgentCallbacks<MockEffectiveConfigLoaderMock>,
        MockEffectiveAgentAssemblerMock,
        MockSupervisorBuilder<MockSupervisorStarter>,
        MockHashRepositoryMock,
        MockYAMLConfigRepositoryMock,
    >;

    fn create_sub_agent_with_mocks() -> SubAgentForTesting {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
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

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(
                predicate::function(move |e: &Result<EffectiveAgent, _>| {
                    e.as_ref().is_ok_and(|x| *x == effective_agent)
                }),
                predicate::always(),
            )
            .returning(|_, _| Ok(None));

        SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            none_mock_opamp_client(),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
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

    // Tests for `build_supervisor_or_default``
    // Essentially, the function `build_supervisor_or_default` defines the behavior for
    // a certain combination of the following parameters:
    //
    // - `maybe_opamp_client`, the presence of an OpAMP client. Can be either `Some(opamp_client)` or `None`.
    // - `hash_repository`, the presence of a hash in the hash repository for the given agent_id: The call to `hash_repository.get(agent_id)?` done inside the function returns either `Some(Hash)` or `None`.
    // - `effective_agent_res`, the result of the agent assembly attempt. Can be either `Ok(EffectiveAgent)` or `Err(EffectiveAgentsAssemblerError)`.
    //
    // When `maybe_opamp_client == None` the function `hash_repository.get(agent_id)?` won't be called, there's no value to check for.
    // We are safe to discard those from the testing set and only look at `effective_agent_res` in this case.
    //
    // So, we cover all cases.

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_ok_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        let mut seq = Sequence::new();
        hash_repository
            .expect_get()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(Some(Hash::new("some_hash".to_string()))));
        hash_repository
            .expect_save()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .once()
            .with(predicate::eq(RemoteConfigStatus {
                last_remote_config_hash: "some_hash".as_bytes().to_vec(),
                status: RemoteConfigStatuses::Applied as i32,
                error_message: "".to_string(),
            }))
            .returning(|_| Ok(()));
        started_opamp_client.should_update_effective_config(1);

        // Actual test
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
            effective_agent,
            |effective_agent| {
                assert_eq!(
                    EffectiveAgent::new(
                        agent_id.clone(),
                        AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
                        Runtime::default()
                    ),
                    effective_agent
                );
                Ok(())
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_err_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        let mut seq = Sequence::new();
        hash_repository
            .expect_get()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(Some(Hash::new("some_hash".to_string()))));
        hash_repository
            .expect_save()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .once()
            .with(predicate::eq(RemoteConfigStatus {
                last_remote_config_hash: "some_hash".as_bytes().to_vec(),
                status: RemoteConfigStatuses::Failed as i32,
                error_message: "error assembling agents: `some_error`".to_string(),
            }))
            .returning(|_| Ok(()));

        // Actual test
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
            |_| Ok(Some(())), // On error, we don't actually call this function and should be using the default for the Option<()> which is None, note we test this below!
        );

        assert!(actual.is_ok());
        assert!(actual.unwrap().is_none());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_ok_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().once().returning(|_| Ok(None));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .never();

        // Actual test
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
            |effective_agent| {
                assert_eq!(
                    EffectiveAgent::new(
                        agent_id.clone(),
                        AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
                        Runtime::default()
                    ),
                    effective_agent
                );
                Ok(())
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_err_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().once().returning(|_| Ok(None));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .never();

        // Actual test
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
            |_| Ok(Some(())), // On error, we don't actually call this function and should be using the default for the Option<()> which is None, note we test this below!
        );

        assert!(actual.is_ok());
        assert!(actual.unwrap().is_none());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_) || hash_repository.get(agent_id)? == None` (it won't be called)
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_ok_eff_agent_no_opamp() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Ok(EffectiveAgent::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            Runtime::default(),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &None,
            effective_agent_res,
            |effective_agent| {
                assert_eq!(
                    EffectiveAgent::new(
                        agent_id.clone(),
                        AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
                        Runtime::default()
                    ),
                    effective_agent
                );
                Ok(())
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_) || hash_repository.get(agent_id)? == None` (it won't be called)
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_err_eff_agent_no_opamp() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &None,
            effective_agent_res,
            |_| Ok(Some(())), // On error, we don't actually call this function, this time, the call to `build_supervisor_or_default` will bubble up the error!
        );

        assert!(actual.is_err());
    }

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
}
