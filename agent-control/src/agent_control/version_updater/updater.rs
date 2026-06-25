use crate::agent_control::config::AgentControlDynamicConfig;
use thiserror::Error;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("update failed: {0}")]
    UpdateFailed(String),
}

impl UpdaterError {
    /// Returns a stable, low-cardinality error code suitable for metric labels.
    /// Derived from keywords in the error message so callers get useful
    /// dimension values without open-ended string cardinality.
    pub fn error_code(&self) -> &'static str {
        let UpdaterError::UpdateFailed(msg) = self;
        if msg.contains("install") {
            "install_failed"
        } else if msg.contains("verify") {
            "verify_failed"
        } else if msg.contains("self replacing") || msg.contains("replace") {
            "replace_failed"
        } else if msg.contains("patch") || msg.contains("HelmRelease") {
            "helm_patch_failed"
        } else if msg.contains("publish") || msg.contains("stop request") {
            "restart_request_failed"
        } else {
            "update_failed"
        }
    }
}

/// A trait for updating the agent control version using a dynamic configuration.
///
/// Implementers of this trait are responsible for notifying an external controller
/// about the desired agent control version, as specified in the provided
/// [`AgentControlDynamicConfig`].
pub trait VersionUpdater {
    /// Verifies if the agent control version should be updated based on the provided configuration and
    /// attempts to update the desired agent control version.
    ///
    /// Returns `Ok(())` if the desired version has been successfully communicated
    /// to the external controller, or an `UpdaterError` if the update fails.
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
}

pub struct NoOpUpdater;

impl VersionUpdater for NoOpUpdater {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        Ok(())
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
        }
    }

    impl MockVersionUpdater {
        /// Returns a mock that always returns `Ok()` regardless of the times it is called
        pub fn new_no_op() -> Self {
            let mut mock = Self::new();
            mock.expect_update().returning(|_| Ok(()));
            mock
        }
    }
}
