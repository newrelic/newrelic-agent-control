use super::channel::EventConsumer;
use crossbeam::channel::RecvTimeoutError;
use std::time::Duration;

pub type CancellationMessage = ();

impl EventConsumer<CancellationMessage> {
    /// Check if the consumer is cancelled.
    /// It returns true if the consumer received a cancellation message or received an error
    /// before the provided timeout is elapsed. Otherwise it blocks until the timeout is elapsed
    /// and returns false.
    pub fn is_cancelled(&self, timeout: Duration) -> bool {
        let timed_out = matches!(
            self.as_ref().recv_timeout(timeout),
            Err(RecvTimeoutError::Timeout)
        );
        !timed_out
    }
}
