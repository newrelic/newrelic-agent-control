use std::marker::PhantomData;
use std::sync::Arc;

use super::health_checker::{HealthChecker, HealthCheckerNotStarted, HealthCheckerStarted};
use super::supervisor::command_supervisor;
use super::supervisor::command_supervisor::SupervisorOnHost;
use crate::agent_type::environment::Environment;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::effective_agents_assembler::{
    EffectiveAgent, EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
};
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{NotStarted, Started};
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use tracing::{debug, error};

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On Host
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentOnHost<'a, S, V, H, A, C, CB, B> {
    supervisor: Option<SupervisorOnHost<V>>,
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    // would make sense to move it to state and share implementation with k8s?
    health_checker: Option<HealthChecker<H>>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    effective_agent_assembler: &'a A,
    maybe_opamp_client: Arc<Option<C>>,
    supervisor_builder: B,
    state: S,

    // This is needed to ensure the generic type parameter CB is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_callbacks: PhantomData<CB>,
}

impl<'a, E, A, C, CB, B>
    SubAgentOnHost<
        'a,
        NotStarted<E>,
        command_supervisor::NotStarted,
        HealthCheckerNotStarted,
        A,
        C,
        CB,
        B,
    >
where
    E: SubAgentEventProcessor,
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<
        Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
        OpAMPClient = C,
    >,
{
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        effective_agent_assembler: &'a A,
        maybe_opamp_client: Arc<Option<C>>,
        supervisor_builder: B,
    ) -> Self {
        Self {
            agent_id,
            agent_cfg,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
            effective_agent_assembler,
            maybe_opamp_client,
            supervisor_builder,
            supervisor: None,
            health_checker: None,
            _opamp_callbacks: PhantomData,
        }
    }
}

impl<S, V, H, A, C, CB, B> SubAgentOnHost<'_, S, V, H, A, C, CB, B>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<
        Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
        OpAMPClient = C,
    >,
{
    fn assemble_agent(&self) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        self.effective_agent_assembler.assemble_agent(
            &self.agent_id,
            &self.agent_cfg,
            &Environment::OnHost,
        )
    }

    fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    ) -> Option<SupervisorOnHost<command_supervisor::NotStarted>> {
        self.supervisor_builder
            .build_supervisor(effective_agent_result, self.maybe_opamp_client.as_ref())
            .inspect_err(
                |err| error!(agent_id=%self.agent_id, %err, "Error building the onhost supervisor"),
            )
            .unwrap_or_default()
    }

    fn start_supervisor(
        &self,
        maybe_not_started_supervisor: Option<SupervisorOnHost<command_supervisor::NotStarted>>,
    ) -> Option<SupervisorOnHost<command_supervisor::Started>> {
        maybe_not_started_supervisor.map(|s| {
            debug!("Running supervisor {} for {}", s.id(), self.agent_id);
            s.run(self.sub_agent_internal_publisher.clone())
        })
    }

    fn stop_supervisor(
        agent_id: &AgentID,
        maybe_started_supervisor: Option<SupervisorOnHost<command_supervisor::Started>>,
    ) {
        if let Some(s) = maybe_started_supervisor {
            let _ = s.stop().join().inspect_err(|_| {
                error!(
                    agent_id = %agent_id,
                    "Error stopping supervisor thread"
                );
            });
        };
    }

    fn build_health_checker(
        &self,
        effective_agent_result: &Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    ) -> Option<HealthChecker<HealthCheckerNotStarted>> {
        effective_agent_result
            .as_ref()
            .ok()?
            .get_onhost_config()
            .inspect_err(|err| {
                error!(
                    %self.agent_id,
                    %err,
                    "could not launch health checker, using default",
                )
            })
            .ok()?
            .health
            .as_ref()
            .and_then(|health_config| {
                HealthChecker::try_new(
                    self.agent_id.clone(),
                    self.sub_agent_internal_publisher.clone(),
                    health_config.clone(),
                )
                .inspect_err(|err| {
                    error!(
                        %self.agent_id,
                        %err,
                        "could not launch health checker, using default",
                    )
                })
                .ok()
            })
    }

    fn start_health_checker(
        maybe_health_checker: Option<HealthChecker<HealthCheckerNotStarted>>,
    ) -> Option<HealthChecker<HealthCheckerStarted>> {
        maybe_health_checker.map(|h| h.start())
    }

    fn stop_health_checker(maybe_health_checker: Option<HealthChecker<HealthCheckerStarted>>) {
        if let Some(health_checker) = maybe_health_checker {
            health_checker.stop();
        }
    }
}

impl<'a, E, A, C, CB, B> NotStartedSubAgent
    for SubAgentOnHost<
        'a,
        NotStarted<E>,
        command_supervisor::NotStarted,
        HealthCheckerNotStarted,
        A,
        C,
        CB,
        B,
    >
where
    E: SubAgentEventProcessor,
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<
        Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
        OpAMPClient = C,
    >,
{
    type StartedSubAgent =
        SubAgentOnHost<'a, Started, command_supervisor::Started, HealthCheckerStarted, A, C, CB, B>;

    fn run(self) -> Self::StartedSubAgent {
        let effective_agent_result = self.assemble_agent();
        let maybe_not_started_health_checker = self.build_health_checker(&effective_agent_result);
        let maybe_not_started_supervisor = self.build_supervisor(effective_agent_result);

        let started_supervisor = self.start_supervisor(maybe_not_started_supervisor);

        let event_loop_handle = self.state.event_processor.process();

        let started_health_checker = Self::start_health_checker(maybe_not_started_health_checker);

        SubAgentOnHost {
            supervisor: started_supervisor,
            agent_id: self.agent_id,
            agent_cfg: self.agent_cfg,
            health_checker: started_health_checker,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
            effective_agent_assembler: self.effective_agent_assembler,
            maybe_opamp_client: self.maybe_opamp_client,
            supervisor_builder: self.supervisor_builder,
            _opamp_callbacks: PhantomData,
        }
    }
}

impl<A, C, CB, B> StartedSubAgent
    for SubAgentOnHost<'_, Started, command_supervisor::Started, HealthCheckerStarted, A, C, CB, B>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<
        Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
        OpAMPClient = C,
    >,
{
    fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }

    fn agent_type(&self) -> AgentTypeFQN {
        self.agent_cfg.agent_type.clone()
    }

    fn stop(self) {
        Self::stop_health_checker(self.health_checker);
        Self::stop_supervisor(&self.agent_id, self.supervisor);

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

    fn apply_config_update(&mut self) {
        // Stop the current supervisor and health checker
        Self::stop_health_checker(self.health_checker.take());
        Self::stop_supervisor(&self.agent_id, self.supervisor.take());
        // Build new supervisor and health checker from persisted values
        let effective_agent_result = self.assemble_agent();
        let maybe_not_started_health_checker = self.build_health_checker(&effective_agent_result);
        let maybe_not_started_supervisor = self.build_supervisor(effective_agent_result);
        // Start the new supervisor and health checker if any
        self.supervisor = self.start_supervisor(maybe_not_started_supervisor);
        self.health_checker = Self::start_health_checker(maybe_not_started_health_checker);
    }
}

#[cfg(test)]
mod test {
    use mockall::{mock, predicate};

    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::on_host::sub_agent::SubAgentOnHost;
    use crate::sub_agent::on_host::supervisor::command_supervisor::SupervisorOnHost;
    use crate::sub_agent::supervisor::SupervisorBuilder;
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use std::thread::sleep;
    use std::time::Duration;

    use super::*;

    // Mock for the OnHost supervisor builder (the associated type needs to be set, therefore we cannot define a generic mock).
    mock! {
        pub SupervisorBuilderOnhost {}

        impl SupervisorBuilder for SupervisorBuilderOnhost {
            type Supervisor = SupervisorOnHost<command_supervisor::NotStarted>;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
                maybe_opamp_client: &Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>>,
            ) -> Result<Option<SupervisorOnHost<command_supervisor::NotStarted>>, SubAgentBuilderError>;
        }
    }

    #[test]
    fn test_events_are_processed() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let mut event_processor = MockEventProcessorMock::default();
        event_processor.should_process();

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();

        let effective_agent = on_host_final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
        );

        let mut supervisor_builder = MockSupervisorBuilderOnhost::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(
                predicate::function(move |e: &Result<EffectiveAgent, _>| {
                    e.as_ref().is_ok_and(|x| *x == effective_agent)
                }),
                predicate::always(),
            )
            .returning(|_, _| Ok(None));

        let sub_agent = SubAgentOnHost::new(
            agent_id,
            agent_cfg,
            event_processor,
            sub_agent_internal_publisher,
            &assembler,
            Arc::new(none_mock_opamp_client()),
            supervisor_builder,
        );
        let started_agent = sub_agent.run();
        //let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        // close the OpAMP Publisher
        started_agent.stop();
    }

    fn on_host_final_agent(agent_id: AgentID, agent_fqn: AgentTypeFQN) -> EffectiveAgent {
        use crate::agent_type::definition::TemplateableValue;

        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost {
                        executable: None,
                        enable_file_logging: TemplateableValue::new(false),
                        health: None,
                    }),
                    k8s: None,
                },
            },
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
