use std::thread::JoinHandle;

use tracing::error;

use crate::{
    agent_control::config::AgentID,
    event::channel::{EventPublisher, EventPublisherError},
};

pub trait SupervisorStopper {
    fn stop(self) -> Result<(), EventPublisherError>;
}

pub struct ThreadResources {
    pub thread_name: String,
    pub stop_publisher: Option<EventPublisher<()>>,
    pub join_handle: JoinHandle<()>,
}

impl ThreadResources {
    pub fn stop(self, agent_id: &AgentID) -> Result<(), EventPublisherError> {
        if let Some(stop_publisher) = self.stop_publisher {
            stop_publisher.publish(())?;
        }
        let _ = self.join_handle.join().inspect_err(|_| {
            error!(
                agent_id = agent_id.to_string(),
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
