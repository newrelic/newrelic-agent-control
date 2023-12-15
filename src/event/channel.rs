use crossbeam::channel::{unbounded, Receiver, Sender};

pub struct EventConsumer<E>(Receiver<E>);
pub struct EventPublisher<E>(Sender<E>);

pub fn pub_sub<E>() -> (EventPublisher<E>, EventConsumer<E>) {
    let (s, r) = unbounded();
    (EventPublisher(s), EventConsumer(r))
}

impl<E> EventPublisher<E> {
    pub fn publish(&self, event: E) {
        // TODO: remove unwrap
        self.0.send(event).unwrap()
    }
}

impl<E> Clone for EventPublisher<E> {
    fn clone(&self) -> Self {
        EventPublisher(self.0.clone())
    }
}

impl<E> AsRef<Receiver<E>> for EventConsumer<E> {
    fn as_ref(&self) -> &Receiver<E> {
        &self.0
    }
}
