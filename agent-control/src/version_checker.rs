pub mod handler;
pub mod k8s;
pub mod onhost;

use crate::event::channel::EventPublisher;
use std::fmt::Debug;
use tracing::error;
const VERSION_CHECKER_THREAD_NAME: &str = "version_checker";

pub trait VersionChecker {
    /// Use it to report the agent version for the opamp client
    /// Uses a thread to check the version of an agent and report it
    /// with internal events. The reported AgentVersion should
    /// contain "version" and the field for opamp that is going to contain the version
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentVersion {
    pub version: String,
    pub opamp_field: String,
}

#[derive(thiserror::Error, Debug)]
#[error("checking version: {0}")]
pub struct VersionCheckError(pub String);

pub(crate) fn publish_version_event<T>(version_event_publisher: &EventPublisher<T>, event: T)
where
    T: Debug + Send + Sync + 'static,
{
    let event_type_str = format!("{event:?}");
    _ = version_event_publisher.publish(event).inspect_err(|e| {
        error!(
            err = e.to_string(),
            event_type = event_type_str,
            "could not publish version event"
        )
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub VersionChecker {}
        impl VersionChecker for VersionChecker {
            fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
        }
    }
}
