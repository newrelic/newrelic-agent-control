use std::sync::mpsc::{Receiver, Sender};
use crate::event::opamp_event::{OpAMPEvent, OpAMPEventHandler};

pub(crate) trait Event {
    fn event_name(&self) -> String;
}

pub(crate) trait EventHandler<T: Event> {
    fn handle(&self, event:T);
}

pub(crate) trait EventConsumer<E: Event> {
    fn consume(&self) -> E;
}

pub(crate) trait EventPublisher<E: Event> {
    fn publish(&self, event: E);
}

impl<E> EventPublisher<E> for Sender<E> {
    fn publish(&self, event: E) {
        self.send(event).unwrap()
    }
}

impl<E: Event> EventConsumer<E> for Receiver<E> {
    // handlers Hashmap<messageType, Handler>
    fn consume(&self,) -> E {
        self.recv().unwrap()
    }
}

struct SubAgent<C = Receiver<OpAMPEvent>, H = OpAMPEventHandler>
    where
        C: EventConsumer<OpAMPEvent>,
        H: EventHandler<OpAMPEvent>,
{
    event_consumer: C,
    opamp_event_handler: H,
}

impl<C, H> SubAgent<C, H>
    where
        C: EventConsumer<OpAMPEvent>,
        H: EventHandler<OpAMPEvent>,
{
    fn new<C, H, P>(event_consumer: C, opamp_event_handler: H) -> Self {
        Self {
            event_consumer,
            opamp_event_handler,
        }
    }
    fn run(&self) {
        loop {
            let event = self.event_consumer.consume();
            self.opamp_event_handler.handle(event);
        }
    }
}
