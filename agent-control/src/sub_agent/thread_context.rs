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

    // Channel to send the stop signal to the thread
    //
    // The stop signal is sent to the thread to stop the infinite loop.
    //
    // There is an exception. `crate::sub_agent::on_host::supervisor::NotStartedSupervisorOnHost::start_process_thread`,
    // which doesn't use this mechanism. We still create the publisher and pass it to the thread but it won't be used.
    stop_publisher: EventPublisher<CancellationMessage>,

    // Handle to the thread
    //
    // All threads run infinitely and are only stopped when a message is published
    // to the `stop_publisher`. Therefore, to stop the thread we need to first publish
    // a message to the `stop_publisher` and then wait for the thread to finish.
    //
    // There is an exception. `crate::sub_agent::on_host::supervisor::NotStartedSupervisorOnHost::start_process_thread`,
    // which doesn't use this mechanism.
    join_handle: JoinHandle<()>,
}

impl StartedThreadContext {
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

    pub fn is_thread_finished(&self) -> bool {
        self.join_handle.is_finished()
    }

    pub fn stop(self) -> Result<(), EventPublisherError> {
        self.stop_publisher.publish(())?;
        let _ = self.join_handle.join().inspect_err(|err| {
            error!(
                agent_id = %self.agent_id,
                err = err.downcast_ref::<&str>().unwrap_or(&"Unknown error"),
                "Error stopping {} thread", self.thread_name
            );
        });
        info!(agent_id = %self.agent_id, "{} stopped", self.thread_name);

        Ok(())
    }
}
