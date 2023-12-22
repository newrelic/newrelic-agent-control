use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent};
use crate::sub_agent::SubAgentCallbacks;
use crossbeam::select;
use opamp_client::StartedClient;
use std::thread;
use std::thread::JoinHandle;
use tracing::debug;

pub struct EventProcessor<C>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
{
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
    maybe_opamp_client: Option<C>,
}

#[cfg_attr(test, mockall::automock)]
impl<C> EventProcessor<C>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
{
    pub fn new(
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        maybe_opamp_client: Option<C>,
    ) -> Self {
        EventProcessor {
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            maybe_opamp_client,
        }
    }

    // process will process the Sub Agent OpAMP events and will return the OpAMP client
    // when processing ends.
    // It will end when sub_agent_opamp_publisher is closed
    pub fn process(self) -> JoinHandle<Option<C>> {
        thread::spawn(move || {
            loop {
                select! {
                    recv(&self.sub_agent_opamp_consumer.as_ref()) -> opamp_event_res => {
                        match opamp_event_res {
                            Err(_) => {
                                debug!("channel closed");
                                break;
                            }
                            Ok(OpAMPEvent::InvalidRemoteConfigReceived(_remote_config_error)) => {
                                debug!("InvalidRemoteConfigReceived");
                            }
                            Ok(OpAMPEvent::ValidRemoteConfigReceived(_remote_config)) => {
                                debug!("ValidRemoteConfigReceived");
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
    use crate::opamp::remote_config::RemoteConfigError::InvalidConfig;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::on_host::event_processor::{EventProcessor, MockEventProcessor};
    use opamp_client::StartedClient;
    use std::thread;

    use crate::sub_agent::SubAgentCallbacks;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    impl<C> MockEventProcessor<C>
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

        let event_processor = EventProcessor::new(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            Some(opamp_client),
        );
        let handle = event_processor.process();

        // publish an event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::InvalidRemoteConfigReceived(InvalidConfig(
                String::from("some"),
                String::from("string"),
            )))
            .unwrap();

        // publish another event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::ValidRemoteConfigReceived(RemoteConfig {
                agent_id: AgentID::new("some-id").unwrap(),
                hash: Hash::new(String::from("some-hash")),
                config_map: ConfigMap::default(),
            }))
            .unwrap();

        // close the OpAMP Publisher
        drop(sub_agent_opamp_publisher);

        handle.join().unwrap().unwrap();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::on_host::event_processor",
            "InvalidRemoteConfigReceived"
        ));
        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::on_host::event_processor",
            "ValidRemoteConfigReceived"
        ));
    }
}
