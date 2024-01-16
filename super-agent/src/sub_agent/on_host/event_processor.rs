use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent};
use crate::opamp::remote_config_hash::HashRepository;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use crossbeam::select;
use opamp_client::StartedClient;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use tracing::{debug, error};

// This trait is meant for testing, there are no multiple implementations expected
// It cannot be doubled as the implementation has a lifetime constraint
pub trait SubAgentEventProcessor<C>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
{
    fn process(self) -> JoinHandle<Option<C>>;
}

pub struct EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub(super) sub_agent_publisher: EventPublisher<SubAgentEvent>,
    pub(super) sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
    pub(super) maybe_opamp_client: Option<C>,
    pub(super) sub_agent_remote_config_hash_repository: Arc<H>,
    pub(super) remote_values_repo: Arc<R>,
}

impl<C, H, R> EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub fn new(
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        maybe_opamp_client: Option<C>,
        sub_agent_remote_config_hash_repository: Arc<H>,
        remote_values_repo: Arc<R>,
    ) -> Self {
        EventProcessor {
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            maybe_opamp_client,
            sub_agent_remote_config_hash_repository,
            remote_values_repo,
        }
    }
}

impl<C, H, R> SubAgentEventProcessor<C> for EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository + Send + Sync + 'static,
    R: ValuesRepository + Send + Sync + 'static,
{
    // process will process the Sub Agent OpAMP events and will return the OpAMP client
    // when processing ends.
    // It will end when sub_agent_opamp_publisher is closed
    fn process(self) -> JoinHandle<Option<C>> {
        thread::spawn(move || {
            loop {
                select! {
                    recv(&self.sub_agent_opamp_consumer.as_ref()) -> opamp_event_res => {
                        match opamp_event_res {
                            Err(_) => {
                                debug!("channel closed");
                                break;
                            }
                            Ok(OpAMPEvent::InvalidRemoteConfigReceived(remote_config_error)) => {
                                debug!("invalid remote config received");
                                if let Err(e) = self.invalid_remote_config(remote_config_error){
                                    error!("error processing invalid remote config: {}",e.to_string())
                                }
                            }
                            Ok(OpAMPEvent::ValidRemoteConfigReceived(remote_config)) => {
                                debug!("valid remote config received");
                                if let Err(e) = self.valid_remote_config(remote_config){
                                     error!("error processing valid remote config: {}",e.to_string())
                                }
                            }
                        }
                    }
                }
            }
            self.maybe_opamp_client
        })
    }
}

#[cfg(test)]
pub mod test {
    use crate::config::super_agent_configs::AgentID;
    use crate::event::channel::pub_sub;
    use crate::event::OpAMPEvent;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig, RemoteConfigError};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::on_host::event_processor::{EventProcessor, SubAgentEventProcessor};
    use mockall::mock;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::StartedClient;
    use serde_yaml::{Mapping, Value};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread;
    use std::thread::JoinHandle;

    use crate::config::agent_type::trivial_value::TrivialValue;
    use crate::config::agent_values::AgentValues;
    use crate::event::SubAgentEvent::ConfigUpdated;
    use crate::opamp::remote_config_hash::test::MockHashRepositoryMock;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::sub_agent::SubAgentCallbacks;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    mock! {
         pub EventProcessorMock<C> {}

        impl<C> SubAgentEventProcessor<C> for EventProcessorMock<C>
        where
            C: StartedClient<SubAgentCallbacks> + 'static,
        {
            fn process(self) -> JoinHandle<Option<C>>;
        }
    }

    impl<C> MockEventProcessorMock<C>
    where
        C: StartedClient<SubAgentCallbacks> + 'static,
    {
        pub fn should_process(&mut self, maybe_client: Option<C>) {
            self.expect_process()
                .once()
                .return_once(move || thread::spawn(move || maybe_client));
        }
    }

    #[traced_test]
    #[test]
    fn test_event_loop_is_closed() {
        let opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        let event_processor = EventProcessor::new(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );
        let handle = event_processor.process();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::on_host::event_processor",
            "channel closed"
        ));
    }

    #[traced_test]
    #[test]
    fn test_valid_config() {
        let opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let mut values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        hash_repository.should_save_hash(&agent_id, &hash);
        values_repository.should_store_remote(
            &agent_id,
            // &AgentValues::new(HashMap::from([(
            //     String::from("some_item"),
            //     TrivialValue::String(String::from("some_value")),
            // )])),
            &AgentValues::new(Value::Mapping(Mapping::from_iter([(
                Value::String("some_item".to_string()),
                Value::String("some_value".to_string()),
            )]))),
        );

        let remote_config = RemoteConfig {
            config_map,
            hash,
            agent_id: agent_id.clone(),
        };

        let event_processor = EventProcessor::new(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );
        let handle = event_processor.process();

        // publish event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::ValidRemoteConfigReceived(remote_config))
            .unwrap();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::on_host::event_processor",
            "valid remote config received",
        ));

        let expected_event = ConfigUpdated(agent_id.clone());
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }

    #[traced_test]
    #[test]
    fn test_invalid_config() {
        let mut opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            error_message: "this is an error message".to_string(),
            status: Failed as i32,
            last_remote_config_hash: "a-hash".as_bytes().to_vec(),
        });

        let remote_config_error = RemoteConfigError::InvalidConfig(
            String::from("a-hash"),
            String::from("this is an error message"),
        );

        let event_processor = EventProcessor::new(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );
        let handle = event_processor.process();

        // publish event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::InvalidRemoteConfigReceived(remote_config_error))
            .unwrap();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::on_host::event_processor",
            "invalid remote config received",
        ));
    }
}
