use crate::agent_control::config::AgentControlDynamicConfig;
use crate::utils::backoff_gate::Suppression;
use thiserror::Error;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("update failed: {0}")]
    UpdateFailed(String),
    /// The previous attempt to upgrade to this version failed; we are deliberately not hitting
    /// the registry again until the cooldown elapses (or the version changes). The message is
    /// derived from the [`Suppression`] *variant* only (not its failure count), so it is
    /// intentionally **stable across polls** and OpAMP `ConfigState::Failed` does not churn.
    #[error("upgrade to {version} suppressed: {}", cooldown_reason(reason))]
    UpdateInCooldown {
        version: String,
        reason: Suppression,
    },
}

/// Domain wording for a suppressed upgrade. Lives here (not in the agnostic gate) because the
/// phrasing — "desired version" — is agent-control/OpAMP vocabulary. Deliberately ignores the
/// failure count so the rendered message stays stable across polls.
fn cooldown_reason(reason: &Suppression) -> &'static str {
    match reason {
        Suppression::InCooldown { .. } => "retrying after previous failure",
        Suppression::CapReached { .. } => {
            "max consecutive failures reached, retrying at the maximum backoff interval"
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

    /// Re-attempts the most recently requested upgrade, if one is still pending. Driven by a
    /// periodic heartbeat so a transient registry outage recovers without a new desired version
    /// being pushed. Defaults to a no-op for updaters without a retry concept.
    fn retry(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
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
