use std::thread::JoinHandle;

use tracing::{error, info};

use crate::{
    agent_control::config::AgentID,
    event::{
        cancellation::CancellationMessage,
        channel::{pub_sub, EventConsumer, EventPublisher, EventPublisherError},
    },
    utils::threads::spawn_named_thread,
};

pub struct NotStartedThreadContext<F, T>
where
    F: FnOnce(EventConsumer<CancellationMessage>) -> T + Send + 'static,
    T: Send + 'static,
{
    agent_id: AgentID,
    thread_name: String,
    callback: F,
}

impl<F, T> NotStartedThreadContext<F, T>
where
    F: FnOnce(EventConsumer<CancellationMessage>) -> T + Send + 'static,
    T: Send + 'static,
{
    pub fn new<S: Into<String>>(agent_id: AgentID, thread_name: S, callback: F) -> Self {
        Self {
            agent_id,
            thread_name: thread_name.into(),
            callback,
        }
    }

    pub fn start(self) -> StartedThreadContext {
        info!(agent_id = %self.agent_id, "{} started", self.thread_name);
        let (stop_publisher, stop_consumer) = pub_sub::<CancellationMessage>();

        StartedThreadContext::new(
            self.agent_id,
            self.thread_name.clone(),
            stop_publisher,
            spawn_named_thread(&self.thread_name, move || {
                (self.callback)(stop_consumer);
            }),
        )
    }
}

pub struct StartedThreadContext {
    agent_id: AgentID,
    thread_name: String,
    stop_publisher: EventPublisher<CancellationMessage>,
    join_handle: JoinHandle<()>,
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadContextStopperError {
    #[error("Error sending stop signal: {0}")]
    EventPublisherError(#[from] EventPublisherError),

    #[error("Error joining thread: {0}")]
    JoinError(String),
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
        agent_id: AgentID,
        thread_name: String,
        stop_publisher: EventPublisher<()>,
        join_handle: JoinHandle<()>,
    ) -> Self {
        Self {
            agent_id,
            thread_name,
            stop_publisher,
            join_handle,
        }
    }

    pub fn get_thread_name(&self) -> &str {
        &self.thread_name
    }

    pub fn stop(self) -> Result<(), ThreadContextStopperError> {
        self.stop_publisher.publish(())?;
        self.join_handle.join().map_err(|err| {
            ThreadContextStopperError::JoinError(
                err.downcast_ref::<&str>()
                    .unwrap_or(&"Unknown error")
                    .to_string(),
            )
        })?;
        info!(agent_id = %self.agent_id, "{} stopped", self.thread_name);

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use crate::{
        agent_control::config::AgentID,
        event::{cancellation::CancellationMessage, channel::EventConsumer},
        sub_agent::thread_context::NotStartedThreadContext,
    };

    use super::StartedThreadContext;

    impl StartedThreadContext {
        pub fn is_thread_finished(&self) -> bool {
            self.join_handle.is_finished()
        }
    }

    #[test]
    fn test_thread_context_start_stop() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let thread_name = "test-thread";
        let callback = |stop_consumer: EventConsumer<CancellationMessage>| loop {
            if stop_consumer.is_cancelled(Duration::default()) {
                break;
            }
        };
        let not_started_thread_context =
            NotStartedThreadContext::new(agent_id, thread_name, callback);
        let started_thread_context = not_started_thread_context.start();
        assert!(!started_thread_context.is_thread_finished());

        started_thread_context.stop().unwrap();
    }
}
