/*use std::thread;
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

impl<C> SubAgent<C>
    where
        C: EventConsumer<OpAMPEvent>,
{
    fn new<C, P>(event_consumer: C, event_publisher: P) -> Self {
        Self {
            event_consumer,
            event_publisher,
        }
    }
    fn run(&self) {
        unimplemented!()
    }
}*/
