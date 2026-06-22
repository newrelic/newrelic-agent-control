use crate::agent_control::config::AgentControlDynamicConfig;
use thiserror::Error;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("update failed: {0}")]
    UpdateFailed(String),
    /// The previous attempt to upgrade to this version failed; we are deliberately not hitting
    /// the registry again until the cooldown elapses (or the version changes). The error message
    /// is intentionally **stable across polls** so OpAMP `ConfigState::Failed` does not churn.
    #[error("upgrade to {version} suppressed: {reason}")]
    UpdateInCooldown {
        version: String,
        reason: CooldownReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownReason {
    /// Within the exponential-backoff cooldown window after a failed attempt.
    Backoff,
    /// Maximum consecutive failures reached; suppressed until the desired version changes.
    CapReached,
}

impl std::fmt::Display for CooldownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CooldownReason::Backoff => f.write_str("retrying after previous failure"),
            CooldownReason::CapReached => {
                f.write_str("max consecutive failures reached, waiting for new desired version")
            }
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
