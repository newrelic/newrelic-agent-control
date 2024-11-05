use crossbeam::channel::never;
use crossbeam::select;
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::SystemTime;

use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use tracing::{debug, error};

use crate::agent_type::environment::Environment;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::config_validator::ConfigValidator;
use crate::sub_agent::effective_agents_assembler::{
    EffectiveAgent, EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
};
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::event_handler::opamp::remote_config::remote_config;
use crate::sub_agent::k8s::NotStartedSupervisorK8s;
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::values::yaml_config_repository::YAMLConfigRepository;

use super::supervisor::log_and_report_unhealthy;
use super::supervisor::StartedSupervisorK8s;

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On K8s
////////////////////////////////////////////////////////////////////////////////////

/// SubAgentK8sStopper is implementing the StartedSubAgent trait.
///
/// It stores the runtime JoinHandle and a SubAgentInternalEvent publisher.
/// It's stored in the super-agent's NotStartedSubAgents collection to be able to call
/// the exposed method Stop that will publish a StopRequested event to the runtime
/// and wait on the JoinHandle for the runtime to finish.
pub struct SubAgentK8sStopper {
    agent_id: AgentID,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    runtime: JoinHandle<Result<(), SubAgentError>>,
}

/// SubAgentK8s is implementing the NotStartedSubAgent trait so only the method run
/// can be called from the SuperAgent to start the runtime and receive a StartedSubAgent
/// that can be stopped
///
/// All its methods are internal and only called from the runtime method that spawns
/// a thread listening to events and acting on them.
pub struct SubAgentK8s<C, CB, A, B, HS, Y> {
    pub(super) agent_id: AgentID,
    pub(super) agent_cfg: SubAgentConfig,
    pub(super) maybe_opamp_client: Option<C>,
    effective_agent_assembler: Arc<A>,
    supervisor_builder: B,
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    pub(super) sub_agent_remote_config_hash_repository: Arc<HS>,
    pub(super) remote_values_repo: Arc<Y>,
    pub(super) config_validator: Arc<ConfigValidator>,

    // This is needed to ensure the generic type parameter CB is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_callbacks: PhantomData<CB>,
}

impl<C, CB, A, B, HS, Y> SubAgentK8s<C, CB, A, B, HS, Y>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisorK8s, OpAMPClient = C>,
    HS: HashRepository,
    Y: YAMLConfigRepository,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        maybe_opamp_client: Option<C>,
        effective_agent_assembler: Arc<A>,
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
    ) -> Self {
        SubAgentK8s {
            agent_id,
            agent_cfg,
            maybe_opamp_client,
            effective_agent_assembler,
            supervisor_builder,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher: internal_pub_sub.0,
            sub_agent_internal_consumer: internal_pub_sub.1,
            sub_agent_remote_config_hash_repository,
            remote_values_repo,
            config_validator,

            _opamp_callbacks: PhantomData,
        }
    }
}

impl<C, CB, A, B, HS, Y> SubAgentK8s<C, CB, A, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisorK8s, OpAMPClient = C>
        + Send
        + Sync
        + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    fn assemble_agent(&self) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        self.effective_agent_assembler.assemble_agent(
            &self.agent_id,
            &self.agent_cfg,
            &Environment::K8s,
        )
    }

    fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    ) -> Option<NotStartedSupervisorK8s> {
        self.supervisor_builder
            .build_supervisor(effective_agent_result, &self.maybe_opamp_client)
            .inspect_err(
                |err| error!(agent_id=%self.agent_id, %err, "Error building the k8s supervisor"),
            )
            .unwrap_or_default()
    }

    fn build_supervisor_from_persisted_values(&self) -> Option<NotStartedSupervisorK8s> {
        let effective_agent_result = self.assemble_agent();
        self.build_supervisor(effective_agent_result)
    }

    fn start_supervisor(
        &self,
        maybe_not_started_supervisor: Option<NotStartedSupervisorK8s>,
    ) -> Option<StartedSupervisorK8s> {
        let start_time = SystemTime::now();
        maybe_not_started_supervisor
            .map(|s| s.start(self.sub_agent_internal_publisher.clone(), start_time))
            .transpose()
            .inspect_err(|err| {
                log_and_report_unhealthy(
                    &self.sub_agent_internal_publisher,
                    err,
                    "starting the k8s resources supervisor failed",
                    start_time,
                )
            })
            .unwrap_or(None)
    }

    fn stop_supervisor(agent_id: &AgentID, maybe_started_supervisor: Option<StartedSupervisorK8s>) {
        if let Some(s) = maybe_started_supervisor {
            let _ = s
                .stop()
                .map(|join_handle| {
                    let _ = join_handle.join().inspect_err(|_| {
                        error!(
                            agent_id = %agent_id,
                            "Error stopping k8s supervisor thread"
                        );
                    });
                })
                .inspect_err(|err| {
                    error!(

                            agent_id = %agent_id,
                            %err,
                            "Error stopping k8s supervisor"
                    );
                });
        }
    }

    fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        thread::spawn(move || {
            let maybe_not_started_supervisor = self.build_supervisor_from_persisted_values();
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
                                Self::stop_supervisor(&self.agent_id, supervisor);
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
                                Self::stop_supervisor(&self.agent_id, supervisor);
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
}

impl<C, CB, A, B, HS, Y> NotStartedSubAgent for SubAgentK8s<C, CB, A, B, HS, Y>
where
    C: opamp_client::StartedClient<CB> + Send + Sync + 'static,
    CB: opamp_client::operation::callbacks::Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisorK8s, OpAMPClient = C>
        + Send
        + Sync
        + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    type StartedSubAgent = SubAgentK8sStopper;

    /// Builds and starts the supervisor (if any) and starts the event processor.
    fn run(self) -> Self::StartedSubAgent {
        let agent_id = self.agent_id.clone();
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let runtime_handle = self.runtime();

        SubAgentK8sStopper {
            agent_id,
            sub_agent_internal_publisher,
            runtime: runtime_handle,
        }
    }
}

impl StartedSubAgent for SubAgentK8sStopper {
    // Stop does not delete directly the CR. It will be the garbage collector doing so if needed.
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

#[cfg(test)]
pub mod test {
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::{SubAgentEvent, SubAgentInternalEvent};
    use crate::k8s::client::MockSyncK8sClient;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::k8s::builder::test::k8s_sample_runtime_config;
    use crate::sub_agent::k8s::NotStartedSupervisorK8s;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{helm_release_type_meta, AgentID, AgentTypeFQN};
    use kube::api::DynamicObject;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::traced_test;

    pub const TEST_AGENT_ID: &str = "k8s-test";
    pub const TEST_GENT_FQN: &str = "ns/test:0.1.2";

    use super::*;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use mockall::{mock, predicate};

    // Mock for the k8s supervisor builder (the associated type needs to be set, therefore we cannot define a generic mock).
    mock! {
        pub SupervisorBuilderK8s {}

        impl SupervisorBuilder for SupervisorBuilderK8s {
            type Supervisor = NotStartedSupervisorK8s;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
                maybe_opamp_client: &Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>>,
            ) -> Result<Option<NotStartedSupervisorK8s>, SubAgentBuilderError>;
        }
    }

    // Testing type to make usage more readable
    type SubAgentK8sForTesting = SubAgentK8s<
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>,
        AgentCallbacks<MockEffectiveConfigLoaderMock>,
        MockEffectiveAgentAssemblerMock,
        MockSupervisorBuilderK8s,
        MockHashRepositoryMock,
        MockYAMLConfigRepositoryMock,
    >;

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_stop() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let started_agent = testing_k8s_sub_agent(
            sub_agent_internal_publisher,
            sub_agent_internal_consumer,
            sub_agent_publisher,
            || None,
        )
        .run();

        started_agent.stop();

        assert!(!logs_contain("ERROR"));
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_fail_stop() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let started_agent = testing_k8s_sub_agent(
            sub_agent_internal_publisher.clone(),
            sub_agent_internal_consumer,
            sub_agent_publisher,
            || None,
        )
        .run();

        // We send a message to end the runtime
        let publishing_result =
            sub_agent_internal_publisher.publish(SubAgentInternalEvent::StopRequested);
        assert!(publishing_result.is_ok());

        // And wait enough time for the runtime to consume the event so the loop broken
        sleep(Duration::from_millis(20));

        started_agent.stop();
        // This error is triggered since we try to send a StopRequested event on a closed channel
        assert!(logs_contain("Error stopping event loop"));
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

        let mut k8s_obj = k8s_sample_runtime_config(true);
        k8s_obj.health = Some(K8sHealthConfig {
            interval: Duration::from_millis(500).into(),
        });

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(|_| Ok(()));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());
        mock_client.expect_get_helm_release().returning(|_| {
            Ok(Some(Arc::new(DynamicObject {
                types: Some(helm_release_type_meta()),
                metadata: Default::default(),
                data: Default::default(),
            })))
        });
        let mock_k8s_client = Arc::new(mock_client);

        let agent_id_for_builder = agent_id.clone();

        let supervisor_fn = move || {
            Some(NotStartedSupervisorK8s::new(
                agent_id_for_builder.clone(),
                agent_fqn.clone(),
                mock_k8s_client.clone(),
                k8s_obj.clone(),
            ))
        };

        // If the started subagent is dropped, then the underlying supervisor is also dropped (and the underlying tasks are stopped)
        let _ = testing_k8s_sub_agent(
            sub_agent_internal_publisher,
            sub_agent_internal_consumer,
            sub_agent_publisher,
            supervisor_fn,
        )
        .run();

        let timeout = Duration::from_secs(3);

        match sub_agent_consumer.as_ref().recv_timeout(timeout).unwrap() {
            SubAgentEvent::SubAgentHealthInfo(_, _, h) => {
                if h.is_healthy() {
                    panic!("unhealthy event expected")
                }
            }
        }
    }

    /// Sets up a k8s sub agent for testing and the corresponding underlying mocks. The supervisor builder will
    /// call the provided `supervisor_fn` to return the corresponding supervisor.
    fn testing_k8s_sub_agent<F: Fn() -> Option<NotStartedSupervisorK8s> + Send + 'static>(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        supervisor_fn: F,
    ) -> SubAgentK8sForTesting {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: agent_fqn.clone(),
        };
        let k8s_config = k8s_sample_runtime_config(true);
        let runtime_config = Runtime {
            deployment: Deployment {
                k8s: Some(k8s_config),
                ..Default::default()
            },
        };
        let effective_agent =
            EffectiveAgent::new(agent_id.clone(), agent_fqn.clone(), runtime_config.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::K8s,
            effective_agent,
            1,
        );

        let mut supervisor_builder = MockSupervisorBuilderK8s::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::always(), predicate::always())
            .returning(move |_, _| Ok(supervisor_fn()));

        let sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        SubAgentK8s::new(
            agent_id.clone(),
            agent_cfg.clone(),
            none_mock_opamp_client(),
            Arc::new(assembler),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (
                sub_agent_internal_publisher.clone(),
                sub_agent_internal_consumer,
            ),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
