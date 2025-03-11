use std::{
    thread::{sleep, JoinHandle},
    time::Duration,
};

const GRACEFUL_STOP_RETRY: u16 = 10;
const GRACEFUL_STOP_RETRY_INTERVAL: Duration = Duration::from_millis(100);

use crate::{
    event::{
        cancellation::CancellationMessage,
        channel::{pub_sub, EventConsumer, EventPublisher},
    },
    utils::threads::spawn_named_thread,
};

pub struct NotStartedThreadContext<F, T>
where
    F: FnOnce(EventConsumer<CancellationMessage>) -> T + Send + 'static,
    T: Send + 'static,
{
    thread_name: String,
    callback: F,
}

impl<F, T> NotStartedThreadContext<F, T>
where
    F: FnOnce(EventConsumer<CancellationMessage>) -> T + Send + 'static,
    T: Send + 'static,
{
    pub fn new<S: Into<String>>(thread_name: S, callback: F) -> Self {
        Self {
            thread_name: thread_name.into(),
            callback,
        }
    }

    pub fn start(self) -> StartedThreadContext {
        let (stop_publisher, stop_consumer) = pub_sub::<CancellationMessage>();

        StartedThreadContext::new(
            self.thread_name.clone(),
            stop_publisher,
            spawn_named_thread(&self.thread_name, move || {
                (self.callback)(stop_consumer);
            }),
        )
    }
}
pub struct StartedThreadContext {
    thread_name: String,
    stop_publisher: EventPublisher<CancellationMessage>,
    join_handle: JoinHandle<()>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ThreadContextStopperError {
    #[error("Error sending stop signal to '{0}' thread: {1}")]
    EventPublisherError(String, String),

    #[error("Error joining '{0}' thread")]
    JoinError(String),

    #[error("Timeout waiting for '{0}' thread to finish")]
    StopTimeout(String),
}

impl StartedThreadContext {
    /// Returns a new `StartedThreadContext`
    ///
    /// At this point the thread is running in the background.
    /// In general, the thread will run until a message is published to the `stop_publisher`.
    /// Therefore, to stop the thread a message is published through the `stop_publisher`.
    ///
    /// # Exceptions
    /// There are exceptions to this rule. Some threads don't use the mechanism of the `stop_publisher`.
    /// In those cases, the channel will still be created but not used inside the thread.
    pub fn new(
        thread_name: String,
        stop_publisher: EventPublisher<()>,
        join_handle: JoinHandle<()>,
    ) -> Self {
        Self {
            thread_name,
            stop_publisher,
            join_handle,
        }
    }

    /// Returns the thread name.
    pub fn thread_name(&self) -> &str {
        &self.thread_name
    }

    /// It sends a stop signal and periodically checks if the thread has finished until
    /// it timeout defined by `GRACEFUL_STOP_RETRY` * `GRACEFUL_STOP_RETRY_INTERVAL`.
    pub fn stop(self) -> Result<(), ThreadContextStopperError> {
        self.stop_publisher.publish(()).map_err(|err| {
            ThreadContextStopperError::EventPublisherError(
                self.thread_name.clone(),
                err.to_string(),
            )
        })?;
        for _ in 0..GRACEFUL_STOP_RETRY {
            if self.join_handle.is_finished() {
                return self.join_handle.join().map_err(|err| {
                    ThreadContextStopperError::JoinError(
                        err.downcast_ref::<&str>()
                            .unwrap_or(&"Unknown error")
                            .to_string(),
                    )
                });
            }
            sleep(GRACEFUL_STOP_RETRY_INTERVAL);
        }

        Err(ThreadContextStopperError::StopTimeout(self.thread_name))
    }

    /// It sends a stop signal and waits until the thread handle is joined.
    pub fn stop_blocking(self) -> Result<(), ThreadContextStopperError> {
        self.stop_publisher.publish(()).map_err(|err| {
            ThreadContextStopperError::EventPublisherError(
                self.thread_name.clone(),
                err.to_string(),
            )
        })?;
        self.join_handle.join().map_err(|err| {
            ThreadContextStopperError::JoinError(
                err.downcast_ref::<&str>()
                    .unwrap_or(&"Unknown error")
                    .to_string(),
            )
        })?;

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use crate::event::{cancellation::CancellationMessage, channel::EventConsumer};
    use crate::utils::thread_context::ThreadContextStopperError;

    use super::{NotStartedThreadContext, StartedThreadContext};

    impl StartedThreadContext {
        pub fn is_thread_finished(&self) -> bool {
            self.join_handle.is_finished()
        }
    }

    #[test]
    fn test_thread_context_start_stop_blocking() {
        let thread_name = "test-thread";
        let callback = |stop_consumer: EventConsumer<CancellationMessage>| loop {
            if stop_consumer.is_cancelled(Duration::default()) {
                break;
            }
        };

        let started_thread_context = NotStartedThreadContext::new(thread_name, callback).start();
        assert!(!started_thread_context.is_thread_finished());
        started_thread_context.stop_blocking().unwrap();

        let started_thread_context = NotStartedThreadContext::new(thread_name, callback).start();
        assert!(!started_thread_context.is_thread_finished());
        started_thread_context.stop().unwrap();
    }

    #[test]
    fn test_fail_stop() {
        let thread_name = "test-thread";
        let never_ending_fn = |_: EventConsumer<CancellationMessage>| {
            sleep(Duration::from_secs(u64::MAX));
        };
        let started_thread_context =
            NotStartedThreadContext::new(thread_name, never_ending_fn).start();

        assert!(!started_thread_context.is_thread_finished());

        assert_eq!(
            started_thread_context.stop().unwrap_err(),
            ThreadContextStopperError::StopTimeout(thread_name.to_string())
        );
    }
}
