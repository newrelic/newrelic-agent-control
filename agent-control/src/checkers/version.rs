//! Checking and reporting the version of a managed agent.
/// Kubernetes version checkers.
pub mod k8s;

use std::fmt::Debug;

const VERSION_CHECKER_THREAD_NAME: &str = "version_checker";

/// A type that can retrieve the version of a managed agent.
pub trait VersionChecker {
    /// Use it to report the agent version for the opamp client
    /// Uses a thread to check the version of an agent and report it
    /// with internal events. The reported AgentVersion should
    /// contain "version" and the field for opamp that is going to contain the version
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
}

/// A version retrieved from an agent, together with the OpAMP field it maps to.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentVersion {
    /// The retrieved agent version.
    pub version: String,
    /// The OpAMP attribute field where the version will be reported.
    pub opamp_field: String,
}

/// Error returned when a version check fails.
#[derive(thiserror::Error, Debug)]
#[error("checking version: {0}")]
pub struct VersionCheckError(
    /// The error message.
    pub String,
);

#[cfg(test)]
#[allow(missing_docs)]
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
