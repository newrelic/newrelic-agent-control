use std::thread::JoinHandle;

use tracing::error;

use crate::{
    agent_control::config::AgentID,
    event::channel::{EventPublisher, EventPublisherError},
};

pub trait SupervisorStopper {
    fn stop(self) -> Result<(), EventPublisherError>;
}

pub struct ThreadContext {
    agent_id: AgentID,
    thread_name: String,

    // Channel to send the stop signal to the thread
    // 
    // The stop signal is sent to the thread to stop the infinite loop.
    // 
    // All threads should have a channel to receive a stop signal, but 
    // method `crate::sub_agent::on_host::supervisor::NotStartedSupervisorOnHost::start_process_thread`
    // doesn't use this mechanism. For this reason, the publisher is optional.
    stop_publisher: Option<EventPublisher<()>>,

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

impl ThreadContext {
    pub fn new(
        agent_id: AgentID,
        thread_name: String,
        stop_publisher: Option<EventPublisher<()>>,
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
        if let Some(stop_publisher) = self.stop_publisher {
            stop_publisher.publish(())?;
        }
        let _ = self.join_handle.join().inspect_err(|err| {
            error!(
                agent_id = %self.agent_id,
                err = err.downcast_ref::<&str>().map_or("Unknown error", |v| v),
                "Error stopping {} thread", self.thread_name
            );
        });

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::event::channel::EventPublisherError;
    use crate::sub_agent::supervisor::stopper::SupervisorStopper;
    use mockall::mock;

    mock! {
        pub SupervisorStopper {}

        impl SupervisorStopper for SupervisorStopper{
        fn stop(self) -> Result<(), EventPublisherError>;
        }
    }
}
