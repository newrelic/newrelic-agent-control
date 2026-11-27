//! Cancellation signalling built on top of the event channel.
//!
//! Models cancellation as an [`EventConsumer`] of [`CancellationMessage`], where receiving a
//! message or a disconnection signals cancellation.

use super::channel::EventConsumer;
use crossbeam::channel::RecvTimeoutError;
use std::time::Duration;

/// The message used to signal cancellation, telling the receiver why it is being cancelled.
///
/// Every reason except [`Self::AgentControlExit`] is imperative: the receiver terminates whatever
/// it manages. On [`Self::AgentControlExit`] the receiver decides whether it keeps running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationMessage {
    /// The work is being replaced by one built from a new configuration.
    ConfigChanged,
    /// The sub-agent was removed.
    SubAgentRemoved,
    /// There is no configuration left to run, so the work is stopped.
    ConfigUnavailable,
    /// The owner of the channel went away without saying why.
    Disconnected,
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
    ///
    /// A disconnection is reported as [`CancellationMessage::Disconnected`], which does not mean
    /// that Agent Control is exiting. Treating it as [`CancellationMessage::AgentControlExit`]
    /// could leave processes running with nothing supervising them while Agent Control keeps
    /// running.
    pub(crate) fn cancellation_with_timeout(
        &self,
        timeout: Duration,
    ) -> Option<CancellationMessage> {
        match self.as_ref().recv_timeout(timeout) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Disconnected) => Some(CancellationMessage::Disconnected),
            Err(RecvTimeoutError::Timeout) => None,
        }
    }
}
