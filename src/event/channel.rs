use crossbeam::channel::{unbounded, Receiver, Sender};

use super::{Consumer, Publisher};

pub struct EventConsumer<E>(Receiver<E>);
pub struct EventPublisher<E>(Sender<E>);

pub fn channel<E>() -> (EventPublisher<E>, EventConsumer<E>) {
    let (s, r) = unbounded();
    (EventPublisher(s), EventConsumer(r))
}

impl<E> Consumer<E> for EventConsumer<E> {
    fn consume(&self) -> E {
        // TODO: remove unwrap
        self.0.recv().unwrap()
    }
}

impl<E> Publisher<E> for EventPublisher<E> {
    fn publish(&self, event: E) {
        // TODO: remove unwrap
        self.0.send(event).unwrap()
    }
}
