use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::AgentID;
use crossbeam::channel::never;
use crossbeam::select;
use opamp_client::StartedClient;
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use tracing::{debug, error};

// This trait is meant for testing, there are no multiple implementations expected
// It cannot be doubled as the implementation has a lifetime constraint
pub trait SubAgentEventProcessor {
    fn process(self) -> JoinHandle<Result<(), SubAgentError>>;
}

pub struct EventProcessor<C, H, R, G>
where
    G: EffectiveConfigLoader + Send + Sync,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    agent_id: AgentID,
    pub(crate) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(crate) sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
    pub(crate) sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
    pub(crate) maybe_opamp_client: Option<C>,
    pub(crate) sub_agent_remote_config_hash_repository: Arc<H>,
    pub(crate) remote_values_repo: Arc<R>,

    _effective_config_loader: PhantomData<G>,
}

impl<C, H, R, G> EventProcessor<C, H, R, G>
where
    G: EffectiveConfigLoader + Send + Sync,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub fn new(
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        maybe_opamp_client: Option<C>,
        sub_agent_remote_config_hash_repository: Arc<H>,
        remote_values_repo: Arc<R>,
    ) -> Self {
        EventProcessor {
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
            sub_agent_remote_config_hash_repository,
            remote_values_repo,

            _effective_config_loader: PhantomData,
        }
    }

    pub(crate) fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }
}

impl<C, H, R, G> SubAgentEventProcessor for EventProcessor<C, H, R, G>
where
    G: EffectiveConfigLoader + Send + Sync + 'static,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository + Send + Sync + 'static,
    R: ValuesRepository + Send + Sync + 'static,
{
    // process will process the Sub Agent OpAMP events and will return the OpAMP client
    // when processing ends.
    // It will end when sub_agent_opamp_publisher is closed
    fn process(self) -> JoinHandle<Result<(), SubAgentError>> {
        thread::spawn(move || {
            debug!(
                agent_id = self.agent_id.to_string(),
                "event processor started"
            );

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
                            Err(_) => {
                                debug!("sub_agent_opamp_consumer :: channel closed");
                                break;
                            }
                            // TODO: the OpAMP flow when a remote configuration is received should be the same
                            // for sub-agent and super-agent:
                            // 1. Remote config is received and an APPLYING message is reported
                            // 2. The configuration is persisted and applied and then an APPLIED message is reported
                            // (considering errors when config is not valid or cannot be applied)
                            // However, the code cannot be the same as of now since the current sub-agent supervisor
                            // will persists the configuration and ask will notify the super-agent which will stop
                            // the current sub-agent supervisor and start a new one which will be in charge of notifying
                            // the APPLIED message.
                            Ok(OpAMPEvent::RemoteConfigReceived(remote_config)) => {
                                debug!("remote config received for: {}", self.agent_id);
                                if let Err(e) = self.remote_config(remote_config){
                                     error!("error processing remote config: {}",e.to_string())
                                }
                            }
                            _ => {}}
                    },
                    recv(&self.sub_agent_internal_consumer.as_ref()) -> sub_agent_internal_event_res => {
                         match sub_agent_internal_event_res {
                            Err(_) => {
                                debug!("sub_agent_internal_consumer :: channel closed");
                                break;
                            }
                            Ok(SubAgentInternalEvent::StopRequested) => {
                                debug!("sub_agent_internal_consumer :: StopRequested");
                                break;
                            },
                            Ok(SubAgentInternalEvent::AgentBecameUnhealthy(unhealthy, start_time))=>{
                                debug!("sub_agent_internal_consumer :: UnhealthyAgent");
                                let _ = self.on_health(HealthWithStartTime::new(unhealthy.into(), start_time)).inspect_err(|e| error!("error processing unhealthy status: {}",e));
                            }
                            Ok(SubAgentInternalEvent::AgentBecameHealthy(healthy, start_time))=>{
                                debug!("sub_agent_internal_consumer :: HealthyAgent");
                                let _ = self.on_health(HealthWithStartTime::new(healthy.into(), start_time)).inspect_err(|e| error!("error processing healthy status: {}",e));
                            }
                         }
                    }
                }
            }
            stop_opamp_client(self.maybe_opamp_client, &self.agent_id)
        })
    }
}

#[cfg(test)]
pub mod test {
    use crate::agent_type::agent_values::AgentValues;
    use crate::event::channel::pub_sub;
    use crate::event::OpAMPEvent;
    use crate::event::SubAgentEvent::ConfigUpdated;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoader;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::{EventProcessor, SubAgentEventProcessor};
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::super_agent::config::AgentID;
    use mockall::mock;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Applying;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread;
    use std::thread::JoinHandle;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    mock! {
         pub EventProcessorMock {}

        impl SubAgentEventProcessor for EventProcessorMock
        {
            fn process(self) -> JoinHandle<Result<(), SubAgentError>>;
        }
    }

    impl MockEventProcessorMock {
        pub fn should_process(&mut self) {
            self.expect_process()
                .once()
                .return_once(move || thread::spawn(|| Ok(())));
        }
    }

    #[traced_test]
    #[test]
    fn test_event_loop_is_closed() {
        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoader>,
        > = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        let event_processor = EventProcessor::new(
            AgentID::new("agent-id").unwrap(),
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );
        let handle = event_processor.process();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::event_processor",
            "channel closed",
        ));
    }

    #[traced_test]
    #[test]
    fn test_remote_config() {
        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoader>,
        > = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let mut values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        hash_repository.should_save_hash(&agent_id, &hash);
        values_repository.should_store_remote(
            &agent_id,
            &AgentValues::new(HashMap::from([("some_item".into(), "some_value".into())])),
        );

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        });
        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        let event_processor = EventProcessor::new(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );
        let handle = event_processor.process();

        // publish event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::event_processor",
            "remote config received",
        ));

        let expected_event = ConfigUpdated(agent_id.clone());
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }
}
