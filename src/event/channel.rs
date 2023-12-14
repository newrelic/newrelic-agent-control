use crossbeam::channel::{unbounded, Receiver, Sender};

use super::{event::Event, EventConsumer, EventPublisher};

impl<E> super::EventPublisher<E> for Sender<E>
where
    E: Send + Sync,
{
    fn publish(&self, event: E) {
        self.send(event).unwrap()
    }
}

impl<E> super::EventConsumer<E> for Receiver<E>
where
    E: Send + Sync,
{
    fn consume(&self) -> E {
        self.recv().unwrap()
    }
}

pub fn event_channel() -> (
    impl EventPublisher<Event> + Clone,
    impl EventConsumer<Event>,
) {
    unbounded()
}
