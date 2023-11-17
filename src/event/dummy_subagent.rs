use std::thread;
use crate::event::event::{EventConsumer, EventPublisher};
use crate::event::opamp_event::{OpAMPEvent, OpAMPEventConsumer};
use crate::event::sub_agent_event::{SubAgentEvent, SubAgentEventPublisher};

struct SubAgent<C = OpAMPEventConsumer, P = SubAgentEventPublisher>
    where
        C: EventConsumer<OpAMPEvent>,
        P: EventPublisher<SubAgentEvent>
{
    event_consumer: C,
    event_publisher: P,
}

impl<C, P> SubAgent<C, P>
    where
        C: EventConsumer<OpAMPEvent>,
        P: EventPublisher<SubAgentEvent>,
{
    fn new(event_consumer: C, event_publisher: P) -> Self {
        Self {
            event_consumer,
            event_publisher,
        }
    }
    fn run(&self) {
        self.event_consumer.consume()
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::sync::mpsc::channel;
    use std::thread::sleep;
    use std::time::Duration;
    use crate::event::opamp_event::{OpAMPEventHandler, RemoteConfig};
    use super::*;


    #[test]
    fn test_consume() {
        let (opamp_sender, opamp_receiver) = channel();
        let (sub_agent_sender, sub_agent_receiver) = channel();

        let opamp_consumer = OpAMPEventConsumer::new(
            Arc::new(Mutex::new(opamp_receiver)),
            Arc::new(Mutex::new(OpAMPEventHandler{})),
        );
        let sub_agent_publisher = SubAgentEventPublisher::new(sub_agent_sender);
        let agent = SubAgent::new(opamp_consumer,sub_agent_publisher);

        thread::spawn(move || {
            loop {
                opamp_sender.send(OpAMPEvent::RemoteConfig(RemoteConfig{ config: "a-config".to_string(), hash: "a-hash".to_string() }));
            }
        });

        agent.run();

        let one_second = Duration::from_secs(3);
        sleep(one_second);

        assert!(true)
    }
}

