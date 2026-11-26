//! Cancellation signalling built on top of the event channel.
//!
//! Models cancellation as an [`EventConsumer`] of [`CancellationMessage`], where receiving a
//! message or a disconnection signals cancellation.

use super::channel::EventConsumer;
use crossbeam::channel::RecvTimeoutError;
use std::time::Duration;

/// The message used to signal cancellation, telling the receiver why it is being cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationMessage {
    /// Stop the work, terminating whatever it manages.
    Stop,
    /// Agent Control is exiting: the receiver decides whether what it manages keeps running.
    AgentControlExit,
}

impl EventConsumer<CancellationMessage> {
    /// Checks whether the consumer is cancelled immediately.
    ///
    /// Calls [`Self::is_cancelled`] with a timeout of zero.
    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled_with_timeout(Duration::ZERO)
    }

    /// Checks whether the consumer is cancelled for the given timeout.
    ///
    /// It returns true if the consumer received a cancellation message or received an error
    /// before the provided timeout is elapsed. Otherwise it blocks until the timeout is elapsed
    /// and returns false.
    pub fn is_cancelled_with_timeout(&self, timeout: Duration) -> bool {
        self.cancellation_with_timeout(timeout).is_some()
    }

    /// Like [`Self::is_cancelled_with_timeout`], returning the cancellation message received.
    /// A disconnection is reported as [`CancellationMessage::Stop`].
    pub(crate) fn cancellation_with_timeout(
        &self,
        timeout: Duration,
    ) -> Option<CancellationMessage> {
        match self.as_ref().recv_timeout(timeout) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Disconnected) => Some(CancellationMessage::Stop),
            Err(RecvTimeoutError::Timeout) => None,
        }
    }
}
