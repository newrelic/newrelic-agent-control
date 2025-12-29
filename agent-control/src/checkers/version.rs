pub mod k8s;
pub mod onhost;

use std::fmt::Debug;

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
