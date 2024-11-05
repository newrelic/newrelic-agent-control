use crossbeam::channel::never;
use crossbeam::select;
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use super::health_checker::{HealthChecker, HealthCheckerNotStarted, HealthCheckerStarted};
use super::supervisor::command_supervisor;
use super::supervisor::command_supervisor::SupervisorOnHost;
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
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::values::yaml_config_repository::YAMLConfigRepository;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use tracing::{debug, error};

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On Host
////////////////////////////////////////////////////////////////////////////////////

/// SubAgentOnHostStopper is implementing the StartedSubAgent trait.
///
/// It stores the runtime JoinHandle and a SubAgentInternalEvent publisher.
/// It's stored in the super-agent's NotStartedSubAgents collection to be able to call
/// the exposed method Stop that will publish a StopRequested event to the runtime
/// and wait on the JoinHandle for the runtime to finish.
pub struct SubAgentOnHostStopper {
    agent_id: AgentID,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    runtime: JoinHandle<Result<(), SubAgentError>>,
}

/// SubAgentOnHost is implementing the NotStartedSubAgent trait so only the method run
/// can be called from the SuperAgent to start the runtime and receive a StartedSubAgent
/// that can be stopped
///
/// All its methods are internal and only called from the runtime method that spawns
/// a thread listening to events and acting on them.
pub struct SubAgentOnHost<A, C, CB, B, HS, Y> {
    pub(super) agent_id: AgentID,
    pub(super) agent_cfg: SubAgentConfig,
    effective_agent_assembler: Arc<A>,
    pub(super) maybe_opamp_client: Option<C>,
    supervisor_builder: B,
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(super) sub_agent_remote_config_hash_repository: Arc<HS>,
    pub(super) remote_values_repo: Arc<Y>,
    pub(super) config_validator: Arc<ConfigValidator>,

    // This is needed to ensure the generic type parameter CB is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_callbacks: PhantomData<CB>,
}

impl<A, C, CB, B, HS, Y> SubAgentOnHost<A, C, CB, B, HS, Y>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    A: EffectiveAgentsAssembler,
    B: SupervisorBuilder<
        Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
        OpAMPClient = C,
    >,
    HS: HashRepository,
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

            _opamp_callbacks: PhantomData,
        }
    }
}

impl<A, C, CB, B, HS, Y> SubAgentOnHost<A, C, CB, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<
            Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
            OpAMPClient = C,
        > + Send
        + Sync
        + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
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
            .build_supervisor(effective_agent_result, &self.maybe_opamp_client)
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

    fn build_health_checker_and_supervisor_from_config(
        &self,
    ) -> (
        Option<SupervisorOnHost<command_supervisor::Started>>,
        Option<HealthChecker<HealthCheckerStarted>>,
    ) {
        // Build new supervisor and health checker from persisted values
        let effective_agent_result = self.assemble_agent();
        let maybe_not_started_health_checker = self.build_health_checker(&effective_agent_result);
        let maybe_not_started_supervisor = self.build_supervisor(effective_agent_result);

        // Start the new supervisor and health checker if any
        (
            self.start_supervisor(maybe_not_started_supervisor),
            Self::start_health_checker(maybe_not_started_health_checker),
        )
    }

    fn runtime(self) -> JoinHandle<Result<(), SubAgentError>> {
        thread::spawn(move || {
            let (mut supervisor, mut health_checker) =
                self.build_health_checker_and_supervisor_from_config();

            debug!(
                agent_id = %self.agent_id,
                "event processor started"
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

                                // Stop the current supervisor and health checker
                                Self::stop_health_checker(health_checker);
                                Self::stop_supervisor(&self.agent_id, supervisor);

                                // Build and start the supervisor and health checker from the new persisted config
                                (supervisor, health_checker) = self.build_health_checker_and_supervisor_from_config();
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
                                Self::stop_health_checker(health_checker);
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
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing health status"));
                            }
                        }
                    }
                }
            }

            stop_opamp_client(self.maybe_opamp_client, &self.agent_id)
        })
    }
}

impl<A, C, CB, B, HS, Y> NotStartedSubAgent for SubAgentOnHost<A, C, CB, B, HS, Y>
where
    C: StartedClient<CB> + Send + Sync + 'static,
    CB: Callbacks + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    B: SupervisorBuilder<
            Supervisor = SupervisorOnHost<command_supervisor::NotStarted>,
            OpAMPClient = C,
        > + Send
        + Sync
        + 'static,
    HS: HashRepository + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    type StartedSubAgent = SubAgentOnHostStopper;

    fn run(self) -> Self::StartedSubAgent {
        let agent_id = self.agent_id.clone();
        let sub_agent_internal_publisher = self.sub_agent_internal_publisher.clone();
        let runtime_handle = self.runtime();

        SubAgentOnHostStopper {
            agent_id,
            sub_agent_internal_publisher,
            runtime: runtime_handle,
        }
    }
}

impl StartedSubAgent for SubAgentOnHostStopper {
    fn stop(self) {
        let _ = self
            .sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)
            .inspect_err(|err| {
                error!(
                    agent_id = %self.agent_id,
                    %err,
                    "Error stopping runtime loop"
                )
            })
            .inspect(|_| {
                let _ = self.runtime.join().inspect_err(|_| {
                    error!(
                        agent_id = %self.agent_id,
                        "Error stopping runtime thread"
                    );
                });
            });
    }
}

#[cfg(test)]
pub(crate) mod test {
    use mockall::{mock, predicate};
    use std::collections::HashMap;

    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::on_host::sub_agent::SubAgentOnHost;
    use crate::sub_agent::on_host::supervisor::command_supervisor::SupervisorOnHost;
    use crate::sub_agent::supervisor::SupervisorBuilder;
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Applying;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

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
    fn test_run_and_stop() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = on_host_final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
            1,
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
        );

        //start the runtime
        let started_agent = sub_agent.run();

        sleep(Duration::from_millis(20));
        // stop the runtime
        started_agent.stop();
    }

    #[traced_test]
    #[test]
    fn test_remote_config() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        let mut sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let mut remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = on_host_final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
            2,
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

        let sub_agent = SubAgentOnHost::new(
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
            "DEBUG newrelic_super_agent::sub_agent::on_host::sub_agent",
            "remote config received",
        ));
    }

    pub(crate) fn on_host_final_agent(
        agent_id: AgentID,
        agent_fqn: AgentTypeFQN,
    ) -> EffectiveAgent {
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
