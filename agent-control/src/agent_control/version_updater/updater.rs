use crate::agent_control::config::AgentControlDynamicConfig;
use thiserror::Error;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("update failed: {0}")]
    UpdateFailed(String),
}

/// A trait for updating the agent control version using a dynamic configuration.
///
/// Implementers of this trait are responsible for notifying an external controller
/// about the desired agent control version, as specified in the provided
/// [`AgentControlDynamicConfig`].
pub trait VersionUpdater {
    /// Attempts to update the desired agent control version.
    ///
    /// Returns `Ok(())` if the desired version has been successfully communicated
    /// to the external controller, or an `UpdaterError` if the update fails.
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;

    /// Verifies if the agent control version should be updated based on the provided configuration.
    /// This method can be used to check if version received from FC is different from the current version
    ///
    /// Returns `Ok(true)` if the version should be updated, `Ok(false)` if no update is needed.
    fn should_update(&self, config: &AgentControlDynamicConfig) -> bool;
}

pub struct NoOpUpdater;

impl VersionUpdater for NoOpUpdater {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        Ok(())
    }

    fn should_update(&self, _config: &AgentControlDynamicConfig) -> bool {
        true
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub VersionUpdater {}
        impl VersionUpdater for VersionUpdater {
            fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
            fn should_update(&self, config: &AgentControlDynamicConfig) -> bool;
        }
    }
}
