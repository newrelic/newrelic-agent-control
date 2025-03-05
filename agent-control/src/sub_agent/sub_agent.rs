use super::error::SubAgentStopError;
use super::health::health_checker::Health;
use crate::agent_control::agent_id::AgentID;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentError};
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::event_handler::on_version::on_version;
use crate::sub_agent::event_handler::opamp::remote_config_handler::RemoteConfigHandler;
use crate::sub_agent::health::health_checker::log_and_report_unhealthy;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crate::utils::threads::spawn_named_thread;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::StartedClient;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

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
pub struct SubAgent<C, SA, R>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigHandler + Send + Sync + 'static,
{
    pub(super) identity: AgentIdentity,
    pub(super) maybe_opamp_client: Option<C>,
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(super) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    remote_config_handler: Arc<R>,
    supervisor_assembler: Arc<SA>,
}

impl<C, SA, R> SubAgent<C, SA, R>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigHandler + Send + Sync + 'static,
{
    pub fn new(
        agent_identity: AgentIdentity,
        maybe_opamp_client: Option<C>,
        supervisor_assembler: Arc<SA>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        internal_pub_sub: (
            EventPublisher<SubAgentInternalEvent>,
            EventConsumer<SubAgentInternalEvent>,
        ),
        remote_config_handler: Arc<R>,
    ) -> Self {
        Self {
            identity: agent_identity,
            maybe_opamp_client,
            supervisor_assembler,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher: internal_pub_sub.0,
            sub_agent_internal_consumer: internal_pub_sub.1,
            remote_config_handler,
        }
    }

    pub fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        spawn_named_thread("SubAgent runtime", move || {
            let mut supervisor = self.assemble_and_start_supervisor();

            // Stores the current healthy state for logging purposes.
            let mut is_healthy = false;

            debug!(
                agent_id = %self.identity.id,
                "runtime started"
            );

            Option::as_ref(&self.maybe_opamp_client).map(|client| client.update_effective_config());

            // The below two lines are used to create a channel that never receives any message
            // if the sub_agent_opamp_consumer is None. Thus, we avoid erroring if there is no
            // publisher for OpAMP events and we attempt to receive them, as erroring while reading
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
            loop {
                select! {
                    recv(opamp_receiver.as_ref()) -> opamp_event_res => {
                        match opamp_event_res {
                            Err(e) => {
                                debug!(error = %e, select_arm = "sub_agent_opamp_consumer", "channel closed");
                                break;
                            }

                            Ok(OpAMPEvent::RemoteConfigReceived(mut config)) => {
                                // This branch only makes sense with a valid OpAMP client
                                let Some(opamp_client) = &self.maybe_opamp_client else {
                                    debug!(select_arm = "sub_agent_opamp_consumer", "got remote config without OpAMP being enabled");
                                    continue;
                                };

                                match self.remote_config_handler.handle(opamp_client,self.identity.clone(),&mut config){
                                    Err(error) =>{
                                        error!(%error,
                                            agent_id = %self.identity.id,
                                            "error handling remote config"
                                        )
                                    },
                                    Ok(())  =>{
                                        info!(agent_id = %self.identity.id, "Applying remote config");
                                        // We need to restart the supervisor after we receive a new config
                                        // as we don't have hot-reloading handling implemented yet
                                        stop_supervisor(&self.identity.id, supervisor);

                                        supervisor = self.assemble_and_start_supervisor();
                                    }
                                }
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
                                stop_supervisor(&self.identity.id, supervisor);
                                break;
                            },
                            Ok(SubAgentInternalEvent::AgentHealthInfo(health))=>{
                                debug!(select_arm = "sub_agent_internal_consumer", ?health, "AgentHealthInfo");
                                Self::log_health_info(&self.identity.id, is_healthy, health.clone().into());
                                let _ = on_health(
                                    health.clone(),
                                    self.maybe_opamp_client.as_ref(),
                                    self.sub_agent_publisher.clone(),
                                    self.identity.clone(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing health message"));
                                is_healthy = health.is_healthy()
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
                }
            }

            stop_opamp_client(self.maybe_opamp_client, &self.identity.id)
        })
    }

    fn log_health_info(agent_id: &AgentID, was_healthy: bool, health: Health) {
        match health {
            // From unhealthy (or initial) to healthy
            Health::Healthy(_) => {
                if !was_healthy {
                    info!(%agent_id, "Agent is healthy");
                }
            }
            // Every time health is unhealthy
            Health::Unhealthy(unhealthy) => {
                warn!(%agent_id, status=unhealthy.status(), last_error=unhealthy.last_error(), "agent is unhealthy");
            }
        }
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
        let stopped_supervisor = self
            .supervisor_assembler
            .assemble_supervisor(&self.maybe_opamp_client,self.identity.clone())
            .inspect_err(
                |e| error!(agent_id = %self.identity.id, agent_type=%self.identity.fqn, error = %e,"cannot assemble supervisor"),
            )
            .ok();

        stopped_supervisor
            .map(|s| self.start_supervisor(s))
            .and_then(|s| s.ok())
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
                "the sub agent thread failed unexpectedly".to_string(),
            )
        })?;
        Ok(runtime_join_result?)
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

impl<C, SA, R> NotStartedSubAgent for SubAgent<C, SA, R>
where
    C: StartedClient + Send + Sync + 'static,
    SA: SupervisorAssembler + Send + Sync + 'static,
    R: RemoteConfigHandler + Send + Sync + 'static,
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

    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::sub_agent::event_handler::opamp::remote_config_handler::tests::MockRemoteConfigHandlerMock;
    use crate::sub_agent::supervisor::assembler::tests::MockSupervisorAssemblerMock;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};
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
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_identity: &AgentIdentity,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent,  SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
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
        MockStartedOpAMPClientMock,
        MockSupervisorAssemblerMock<MockSupervisorStarter>,
        MockRemoteConfigHandlerMock,
    >;

    impl Default for SubAgentForTesting {
        fn default() -> Self {
            let agent_identity = AgentIdentity::from((
                AgentID::new("some-agent-id").unwrap(),
                AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            ));

            let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
            let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

            let mut hash_repository = MockHashRepositoryMock::default();
            hash_repository
                .expect_get()
                .with(predicate::eq(agent_identity.id.clone()))
                .return_const(Ok(None));

            let remote_config_handler = MockRemoteConfigHandlerMock::new();

            let mut started_supervisor = MockSupervisorStopper::new();
            started_supervisor.should_stop();

            let mut stopped_supervisor = MockSupervisorStarter::new();
            stopped_supervisor.should_start(started_supervisor);

            let mut supervisor_assembler = MockSupervisorAssemblerMock::new();
            supervisor_assembler.should_assemble::<MockStartedOpAMPClientMock>(
                stopped_supervisor,
                agent_identity.clone(),
            );

            SubAgent::new(
                agent_identity,
                None,
                Arc::new(supervisor_assembler),
                sub_agent_publisher,
                None,
                (sub_agent_internal_publisher, sub_agent_internal_consumer),
                Arc::new(remote_config_handler),
            )
        }
    }

    #[traced_test]
    #[test]
    fn test_run_and_stop() {
        let sub_agent = SubAgentForTesting::default();
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

        let mut opamp_client = MockStartedOpAMPClientMock::new();
        opamp_client
            .expect_update_effective_config()
            .times(1)
            .returning(|| Ok(()));

        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        // Assemble once on start
        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        let mut supervisor_assembler = MockSupervisorAssemblerMock::new();

        supervisor_assembler.should_assemble::<MockStartedOpAMPClientMock>(
            stopped_supervisor,
            agent_identity.clone(),
        );

        // Receive a remote config
        let mut remote_config_handler = MockRemoteConfigHandlerMock::new();
        remote_config_handler.should_handle::<MockStartedOpAMPClientMock>(
            agent_identity.clone(),
            remote_config.clone(),
        );

        // Assemble again on config received
        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        supervisor_assembler.should_assemble::<MockStartedOpAMPClientMock>(
            stopped_supervisor,
            agent_identity.clone(),
        );

        let sub_agent = SubAgent::new(
            agent_identity,
            Some(opamp_client),
            Arc::new(supervisor_assembler),
            sub_agent_publisher,
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(remote_config_handler),
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
}
