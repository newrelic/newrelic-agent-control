use std::{
    thread::{sleep, JoinHandle},
    time::Duration,
};
use tracing::trace;

const GRACEFUL_STOP_RETRY: u16 = 10;
const GRACEFUL_STOP_RETRY_INTERVAL: Duration = Duration::from_millis(100);

use crate::{
    event::{
        cancellation::CancellationMessage,
        channel::{pub_sub, EventConsumer, EventPublisher},
    },
    utils::threads::spawn_named_thread,
};

pub struct NotStartedThreadContext<F, T = ()>
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

    pub fn start(self) -> StartedThreadContext<T> {
        let (stop_publisher, stop_consumer) = pub_sub::<CancellationMessage>();

        StartedThreadContext::new(
            self.thread_name.clone(),
            stop_publisher,
            spawn_named_thread(&self.thread_name, move || (self.callback)(stop_consumer)),
        )
    }
}
pub struct StartedThreadContext<T = ()>
where
    T: Send + 'static,
{
    thread_name: String,
    stop_publisher: EventPublisher<CancellationMessage>,
    join_handle: JoinHandle<T>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ThreadContextStopperError {
    #[error("Error sending stop signal to '{thread}' thread: {error}")]
    EventPublisherError { thread: String, error: String },

    #[error("Error joining '{thread}' thread: {error}")]
    JoinError { thread: String, error: String },

    #[error("Timeout waiting for '{thread}' thread to finish")]
    StopTimeout { thread: String },
}

impl<T> StartedThreadContext<T>
where
    T: Send + 'static,
{
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
        join_handle: JoinHandle<T>,
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

    fn join_thread(self) -> Result<T, ThreadContextStopperError> {
        self.join_handle
            .join()
            .map_err(|err| ThreadContextStopperError::JoinError {
                thread: self.thread_name,
                error: err
                    .downcast_ref::<&str>()
                    .unwrap_or(&"Unknown error")
                    .to_string(),
            })
    }

    /// It sends a stop signal and periodically checks if the thread has finished until
    /// it timeout defined by `GRACEFUL_STOP_RETRY` * `GRACEFUL_STOP_RETRY_INTERVAL`.
    pub fn stop(self) -> Result<T, ThreadContextStopperError> {
        trace!(thread = self.thread_name, "stopping");
        if self.join_handle.is_finished() {
            trace!(thread = self.thread_name, "finished already, joining");
            return self.join_thread();
        }
        trace!(thread = self.thread_name, "publishing stop");
        self.stop_publisher.publish(()).map_err(|err| {
            ThreadContextStopperError::EventPublisherError {
                thread: self.thread_name.clone(),
                error: err.to_string(),
            }
        })?;
        for _ in 0..GRACEFUL_STOP_RETRY {
            if self.join_handle.is_finished() {
                trace!(thread = self.thread_name, "finished, joining");
                return self.join_thread();
            }
            sleep(GRACEFUL_STOP_RETRY_INTERVAL);
        }

        Err(ThreadContextStopperError::StopTimeout {
            thread: self.thread_name,
        })
    }

    /// It sends a stop signal and waits until the thread handle is joined.
    pub fn stop_blocking(self) -> Result<T, ThreadContextStopperError> {
        trace!(thread = self.thread_name, "stopping");
        if self.join_handle.is_finished() {
            trace!(thread = self.thread_name, "finished already, joining");
            return self.join_thread();
        }
        trace!(thread = self.thread_name, "publishing stop");
        self.stop_publisher.publish(()).map_err(|err| {
            ThreadContextStopperError::EventPublisherError {
                thread: self.thread_name.clone(),
                error: err.to_string(),
            }
        })?;
        trace!(thread = self.thread_name, "joining");
        self.join_thread()
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
    fn test_thread_context_start_stop_blocking_on_finished_thread() {
        let thread_name = "test-thread";
        let callback = |_stop_consumer: EventConsumer<CancellationMessage>| {
            // only to point out that
            print!("this thread will finish after this and _stop_consummer be disconnected")
        };

        let started_thread_context = NotStartedThreadContext::new(thread_name, callback).start();
        assert!(!started_thread_context.is_thread_finished());
        // wait for the thread to finish
        sleep(Duration::from_millis(100));
        started_thread_context.stop_blocking().unwrap();

        let started_thread_context = NotStartedThreadContext::new(thread_name, callback).start();
        assert!(!started_thread_context.is_thread_finished());
        // wait for the thread to finish
        sleep(Duration::from_millis(100));
        started_thread_context.stop().unwrap();
    }

    #[test]
    fn test_thread_context_joins_panic_thread() {
        let thread_name = "test-thread";
        let callback = |_stop_consumer: EventConsumer<CancellationMessage>| {
            panic!("#### EXPECTED PANIC!!! ####")
        };

        let started_thread_context = NotStartedThreadContext::new(thread_name, callback).start();
        // wait for the thread to panic
        sleep(Duration::from_millis(100));
        assert_eq!(
            started_thread_context.stop().unwrap_err(),
            ThreadContextStopperError::JoinError {
                thread: thread_name.to_string(),
                error: "#### EXPECTED PANIC!!! ####".to_string()
            }
        );
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
            ThreadContextStopperError::StopTimeout {
                thread: thread_name.to_string()
            }
        );
    }
}
