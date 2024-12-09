use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentError};
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::health::health_checker::log_and_report_unhealthy;
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::values::yaml_config_repository::YAMLConfigRepository;

use crate::sub_agent::event_handler::opamp::remote_config_handler::RemoteConfigHandler;
use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use std::marker::PhantomData;
use std::thread;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{debug, error};

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
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(super) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    remote_config_handler: RemoteConfigHandler<HS, Y>,
    supervisor_assembler: SupervisorAssembler<HS, B, A>,

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
        maybe_opamp_client: Option<C>,
        supervisor_assembler: SupervisorAssembler<HS, B, A>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        internal_pub_sub: (
            EventPublisher<SubAgentInternalEvent>,
            EventConsumer<SubAgentInternalEvent>,
        ),
        remote_config_handler: RemoteConfigHandler<HS, Y>,
    ) -> Self {
        Self {
            agent_id,
            agent_cfg,
            maybe_opamp_client,
            supervisor_assembler,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_publisher: internal_pub_sub.0,
            sub_agent_internal_consumer: internal_pub_sub.1,
            remote_config_handler,
            _opamp_callbacks: PhantomData,
        }
    }

    pub fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        thread::spawn(move || {
            let mut supervisor = self.assemble_and_start_supervisor();

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
                                // This branch only makes sense with a valid OpAMP client
                                let Some(opamp_client) = &self.maybe_opamp_client else {
                                    debug!(select_arm = "sub_agent_opamp_consumer", "got remote config without OpAMP being enabled");
                                    continue;
                                };

                                match self.remote_config_handler.handle(opamp_client, &mut config){
                                    Err(error) =>{
                                        error!(%error,
                                            agent_id = %self.agent_id,
                                            "error handling remote config"
                                        )
                                    },
                                    Ok(())  =>{
                                        // We need to restart the supervisor after we receive a new config
                                        // as we don't have hot-reloading handling implemented yet
                                        stop_supervisor(&self.agent_id, supervisor);

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

    pub(crate) fn start_supervisor(
        &self,
        not_started_supervisor: B::SupervisorStarter,
    ) -> Result<
        <B::SupervisorStarter as SupervisorStarter>::SupervisorStopper,
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
    ) -> Option<<B::SupervisorStarter as SupervisorStarter>::SupervisorStopper> {
        let stopped_supervisor = self
            .supervisor_assembler
            .assemble_supervisor(&self.maybe_opamp_client)
            .inspect_err(
                |e| error!(agent_id = %self.agent_id, error = %e,"cannot assemble supervisor"),
            )
            .ok();

        stopped_supervisor
            .map(|s| self.start_supervisor(s))
            .and_then(|s| s.ok())
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
pub mod tests {
    use super::*;
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::ConfigValidator;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;

    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
    use mockall::{mock, predicate};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Applying;
    use std::collections::HashMap;
    use std::sync::Arc;
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

            let mut hash_repository = MockHashRepositoryMock::default();
            hash_repository
                .expect_get()
                .with(predicate::eq(agent_id.clone()))
                .return_const(Ok(None));
            let remote_values_repo = MockYAMLConfigRepositoryMock::default();

            let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
            let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
            effective_agent_assembler.should_assemble_agent(
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

            let hash_repository_ref = Arc::new(hash_repository);

            let remote_config_handler = RemoteConfigHandler::new(
                Arc::new(
                    ConfigValidator::try_new()
                        .expect("Failed to compile config validation regexes"),
                ),
                agent_id.clone(),
                agent_cfg.clone(),
                hash_repository_ref.clone(),
                Arc::new(remote_values_repo),
            );

            let supervisor_assembler = SupervisorAssembler::new(
                hash_repository_ref,
                supervisor_builder,
                agent_id.clone(),
                agent_cfg.clone(),
                Arc::new(effective_agent_assembler),
                Environment::OnHost,
            );

            SubAgent::new(
                agent_id,
                agent_cfg,
                None,
                supervisor_assembler,
                sub_agent_publisher,
                None,
                (sub_agent_internal_publisher, sub_agent_internal_consumer),
                remote_config_handler,
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

        let mut hash_repository = MockHashRepositoryMock::default();
        let mut remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
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

        hash_repository.should_save_hash(&agent_id, &hash);
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

        let hash_repository_ref = Arc::new(hash_repository);

        let remote_config_handler = RemoteConfigHandler::new(
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            agent_id.clone(),
            agent_cfg.clone(),
            hash_repository_ref.clone(),
            Arc::new(remote_values_repo),
        );

        let supervisor_assembler = SupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Some(opamp_client),
            supervisor_assembler,
            sub_agent_publisher,
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            remote_config_handler,
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
