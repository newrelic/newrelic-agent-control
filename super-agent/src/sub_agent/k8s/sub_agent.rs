use std::marker::PhantomData;
use std::sync::Arc;
use std::time::SystemTime;

use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use tracing::error;

use crate::agent_type::environment::Environment;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::effective_agents_assembler::{
    EffectiveAgent, EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
};
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::k8s::NotStartedSupervisor;
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{NotStarted, Started};
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig};

use super::supervisor::log_and_report_unhealthy;
use super::supervisor::StartedSupervisor;

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On K8s
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentK8s<'a, S, V, C, CB, A, B> {
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    supervisor: Option<V>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    maybe_opamp_client: Arc<Option<C>>,
    effective_agent_assembler: &'a A,
    supervisor_builder: B,
    state: S,

    // This is needed to ensure the generic type parameter CB is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_callbacks: PhantomData<CB>,
}

impl<'a, E, C, CB, A, B> SubAgentK8s<'a, NotStarted<E>, StartedSupervisor, C, CB, A, B>
where
    E: SubAgentEventProcessor,
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisor, OpAMPClient = C>,
{
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        maybe_opamp_client: Arc<Option<C>>,
        effective_agent_assembler: &'a A,
        supervisor_builder: B,
    ) -> Self {
        SubAgentK8s {
            agent_id,
            agent_cfg,
            supervisor: None,
            sub_agent_internal_publisher,
            maybe_opamp_client,
            effective_agent_assembler,
            supervisor_builder,
            state: NotStarted { event_processor },
            _opamp_callbacks: PhantomData,
        }
    }
}

impl<S, C, CB, A, B> SubAgentK8s<'_, S, StartedSupervisor, C, CB, A, B>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisor, OpAMPClient = C>,
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
    ) -> Option<NotStartedSupervisor> {
        self.supervisor_builder
            .build_supervisor(effective_agent_result, self.maybe_opamp_client.as_ref())
            .inspect_err(
                |err| error!(agent_id=%self.agent_id, %err, "Error building the k8s supervisor"),
            )
            .unwrap_or_default()
    }

    fn build_supervisor_from_persisted_values(&self) -> Option<NotStartedSupervisor> {
        let effective_agent_result = self.assemble_agent();
        self.build_supervisor(effective_agent_result)
    }

    fn start_supervisor(
        &self,
        maybe_not_started_supervisor: Option<NotStartedSupervisor>,
    ) -> Option<StartedSupervisor> {
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

    fn stop_supervisor(agent_id: &AgentID, maybe_started_supervisor: Option<StartedSupervisor>) {
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
}

impl<'a, E, C, CB, A, B> NotStartedSubAgent
    for SubAgentK8s<'a, NotStarted<E>, StartedSupervisor, C, CB, A, B>
where
    E: SubAgentEventProcessor,
    C: opamp_client::StartedClient<CB>,
    CB: opamp_client::operation::callbacks::Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisor, OpAMPClient = C>,
{
    type StartedSubAgent = SubAgentK8s<'a, Started, StartedSupervisor, C, CB, A, B>;

    /// Builds and starts the supervisor (if any) and starts the event processor.
    fn run(self) -> Self::StartedSubAgent {
        let maybe_not_started_supervisor = self.build_supervisor_from_persisted_values();
        let maybe_started_supervisor = self.start_supervisor(maybe_not_started_supervisor);

        let event_loop_handle = self.state.event_processor.process();

        SubAgentK8s {
            agent_id: self.agent_id,
            agent_cfg: self.agent_cfg,
            supervisor: maybe_started_supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            maybe_opamp_client: self.maybe_opamp_client,
            effective_agent_assembler: self.effective_agent_assembler,
            supervisor_builder: self.supervisor_builder,
            state: Started { event_loop_handle },
            _opamp_callbacks: PhantomData,
        }
    }
}

impl<C, CB, A, B> StartedSubAgent for SubAgentK8s<'_, Started, StartedSupervisor, C, CB, A, B>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<Supervisor = NotStartedSupervisor, OpAMPClient = C>,
{
    fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }

    fn agent_type(&self) -> AgentTypeFQN {
        self.agent_cfg.agent_type.clone()
    }

    fn apply_config_update(&mut self) {
        // Stop the current supervisor if any
        let maybe_current_supervisor = self.supervisor.take();
        Self::stop_supervisor(&self.agent_id, maybe_current_supervisor);
        // Build a new supervisor from the persisted values
        let maybe_not_started_supervisor = self.build_supervisor_from_persisted_values();
        // Start the new supervisor if any
        self.supervisor = self.start_supervisor(maybe_not_started_supervisor);
    }

    // Stop does not delete directly the CR. It will be the garbage collector doing so if needed.
    fn stop(self) {
        // stop the k8s object supervisor
        Self::stop_supervisor(&self.agent_id, self.supervisor);

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
                let _ = self.state.event_loop_handle.join().inspect_err(|_| {
                    error!(
                        agent_id = %self.agent_id,
                        "Error stopping event thread"
                    );
                });
            });

        // Stop the OpAMP client in case it wasn't previously stopped by the event handler
        if let Some(maybe_opamp_client) = Arc::into_inner(self.maybe_opamp_client) {
            let _ = stop_opamp_client(maybe_opamp_client, &self.agent_id).inspect_err(|err| {
                error!(agent_id= %self.agent_id, %err, "Error stopping the OpAMP client");
            });
        }
    }
}

#[cfg(test)]
pub mod test {
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::SubAgentInternalEvent;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::k8s::builder::test::k8s_sample_runtime_config;
    use crate::sub_agent::k8s::sub_agent::SubAgentK8s;
    use crate::sub_agent::k8s::NotStartedSupervisor;
    use crate::sub_agent::{NotStarted, NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{helm_release_type_meta, AgentID, AgentTypeFQN};
    use kube::api::DynamicObject;
    use std::sync::Arc;
    use std::time::Duration;
    use tracing_test::traced_test;

    pub const TEST_AGENT_ID: &str = "k8s-test";
    pub const TEST_GENT_FQN: &str = "ns/test:0.1.2";

    use super::*;
    use mockall::{mock, predicate};

    // Mock for the k8s supervisor builder (the associated type needs to be set, therefore we cannot define a generic mock).
    mock! {
        pub SupervisorBuilderK8s {}

        impl SupervisorBuilder for SupervisorBuilderK8s {
            type Supervisor = NotStartedSupervisor;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
                maybe_opamp_client: &Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>>,
            ) -> Result<Option<NotStartedSupervisor>, SubAgentBuilderError>;
        }
    }

    // Testing type to make usage more readable
    type SubAgentK8sForTesting<'a> = SubAgentK8s<
        'a,
        NotStarted<MockEventProcessorMock>,
        StartedSupervisor,
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>,
        AgentCallbacks<MockEffectiveConfigLoaderMock>,
        MockEffectiveAgentAssemblerMock,
        MockSupervisorBuilderK8s,
    >;

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_stop() {
        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();

        let mut assembler = MockEffectiveAgentAssemblerMock::new();

        let started_agent =
            testing_k8s_sub_agent(sub_agent_internal_publisher, &mut assembler, || None).run();

        started_agent.stop();

        assert!(!logs_contain("ERROR"));
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_fail_stop() {
        let (sub_agent_internal_publisher, _) = pub_sub();

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        let started_agent =
            testing_k8s_sub_agent(sub_agent_internal_publisher, &mut assembler, || None).run();

        started_agent.stop();
        // This error is triggered since the consumer is dropped and therefore the channel is closed
        // Therefore, the subAgent fails to write to such channel when stopping
        assert!(logs_contain("Error stopping event loop"));
    }

    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

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

        let mut assembler = MockEffectiveAgentAssemblerMock::new();

        let agent_id_for_builder = agent_id.clone();

        let supervisor_fn = move || {
            Some(NotStartedSupervisor::new(
                agent_id_for_builder.clone(),
                agent_fqn.clone(),
                mock_k8s_client.clone(),
                k8s_obj.clone(),
            ))
        };

        // If the started subagent is dropped, then the underlying supervisor is also dropped (and the underlying tasks are stopped)
        let _started_subagent =
            testing_k8s_sub_agent(sub_agent_internal_publisher, &mut assembler, supervisor_fn)
                .run();

        let timeout = Duration::from_secs(3);

        match sub_agent_internal_consumer
            .as_ref()
            .recv_timeout(timeout)
            .unwrap()
        {
            SubAgentInternalEvent::AgentBecameUnhealthy(_, _) => {}
            _ => {
                panic!("AgentBecameUnhealthy event expected")
            }
        }
    }

    /// Sets up a k8s sub agent for testing and the corresponding underlying mocks. The supervisor builder will
    /// call the provided `supervisor_fn` to return the corresponding supervisor.
    fn testing_k8s_sub_agent<F: Fn() -> Option<NotStartedSupervisor> + Send + 'static>(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        assembler: &mut MockEffectiveAgentAssemblerMock,
        supervisor_fn: F,
    ) -> SubAgentK8sForTesting<'_> {
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
        assembler.should_assemble_agent(&agent_id, &agent_cfg, &Environment::K8s, effective_agent);

        let mut processor = MockEventProcessorMock::new();
        processor.should_process();

        let mut supervisor_builder = MockSupervisorBuilderK8s::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::always(), predicate::always())
            .returning(move |_, _| Ok(supervisor_fn()));

        SubAgentK8s::new(
            agent_id.clone(),
            agent_cfg.clone(),
            processor,
            sub_agent_internal_publisher.clone(),
            Arc::new(none_mock_opamp_client()),
            assembler,
            supervisor_builder,
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
