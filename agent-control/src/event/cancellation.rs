use super::channel::EventConsumer;
use crossbeam::channel::RecvTimeoutError;
use std::time::Duration;

pub type CancellationMessage = ();

impl EventConsumer<CancellationMessage> {
    /// Checks whether the consumer is cancelled for the given timeout.
    ///
    /// It returns true if the consumer received a cancellation message or received an error
    /// before the provided timeout is elapsed. Otherwise it blocks until the timeout is elapsed
    /// and returns false.
    pub fn is_cancelled(&self, timeout: Duration) -> bool {
        match self.as_ref().recv_timeout(timeout) {
            Ok(_) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => true,
        }
    }

    /// Checks whether the consumer is cancelled immediately.
    ///
    /// Calls [`Self::is_cancelled`] with a timeout of zero.
    pub fn is_cancelled_immediately(&self) -> bool {
        self.is_cancelled(Duration::ZERO)
    }
}
