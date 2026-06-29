//! Lightweight publish/subscribe channel wrappers used to pass events between components.

use crossbeam::channel::{Receiver, Sender, unbounded};
use thiserror::Error;

/// The consuming end of an event channel, receiving events of type `E`.
pub struct EventConsumer<E>(Receiver<E>);

impl<E> From<Receiver<E>> for EventConsumer<E> {
    fn from(value: Receiver<E>) -> Self {
        Self(value)
    }
}
/// The producing end of an event channel, publishing events of type `E`.
pub struct EventPublisher<E>(Sender<E>);

impl<E> From<Sender<E>> for EventPublisher<E> {
    fn from(value: Sender<E>) -> Self {
        Self(value)
    }
}

/// Errors that can occur when publishing an event through an [`EventPublisher`].
#[derive(Debug, Error, PartialEq)]
pub enum EventPublisherError {
    /// Publishing an event over the channel failed.
    #[error("error while publishing event: {0}")]
    SendError(String),
}

/// Creates a connected [`EventPublisher`]/[`EventConsumer`] pair backed by an unbounded channel.
pub fn pub_sub<E>() -> (EventPublisher<E>, EventConsumer<E>) {
    let (s, r) = unbounded();
    (EventPublisher(s), EventConsumer(r))
}

impl<E> EventPublisher<E> {
    /// Publishes an event, blocking if the underlying channel is full.
    pub fn publish(&self, event: E) -> Result<(), EventPublisherError> {
        self.0
            .send(event)
            .map_err(|err| EventPublisherError::SendError(err.to_string()))
    }
    /// Attempts to publish an event without blocking, failing if it cannot be sent immediately.
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
