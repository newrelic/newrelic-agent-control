use crossbeam::channel::{Receiver, Sender, unbounded};
use thiserror::Error;

pub struct EventConsumer<E>(Receiver<E>);

impl<E> From<Receiver<E>> for EventConsumer<E> {
    fn from(value: Receiver<E>) -> Self {
        Self(value)
    }
}
pub struct EventPublisher<E>(Sender<E>);

impl<E> From<Sender<E>> for EventPublisher<E> {
    fn from(value: Sender<E>) -> Self {
        Self(value)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum EventPublisherError {
    #[error("error while publishing event: {0}")]
    SendError(String),
}

pub fn pub_sub<E>() -> (EventPublisher<E>, EventConsumer<E>) {
    let (s, r) = unbounded();
    (EventPublisher(s), EventConsumer(r))
}

impl<E> EventPublisher<E> {
    pub fn publish(&self, event: E) -> Result<(), EventPublisherError> {
        self.0
            .send(event)
            .map_err(|err| EventPublisherError::SendError(err.to_string()))
    }
    pub fn try_publish(&self, event: E) -> Result<(), EventPublisherError> {
        self.0
            .try_send(event)
            .map_err(|err| EventPublisherError::SendError(err.to_string()))
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
