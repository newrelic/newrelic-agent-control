//! The [`VersionUpdater`] trait, its error type and a no-op implementation.

use crate::agent_control::config::AgentControlDynamicConfig;
use crate::utils::backoff_gate::SuppressionReason;
use thiserror::Error;

/// Represents errors that can occur during the update process of the agent control version.
#[derive(Debug, Error)]
pub enum UpdaterError {
    /// The update could not be applied.
    #[error("update failed: {0}")]
    UpdateFailed(String),
    /// The previous attempt to upgrade to this version failed; we are deliberately not hitting
    /// the registry again until the cooldown elapses (or the version changes). The message is
    /// derived from the [`SuppressionReason`] *variant* only (not its failure count), so it is
    /// intentionally **stable across polls** and OpAMP `ConfigState::Failed` does not churn.
    #[error("upgrade to {version} suppressed: {}", cooldown_reason(reason))]
    UpdateInCooldown {
        /// The desired version whose upgrade is being suppressed.
        version: String,
        /// Why the upgrade is currently suppressed.
        reason: SuppressionReason,
    },
}

/// Domain wording for a suppressed upgrade. Lives here (not in the agnostic gate) because the
/// phrasing — "desired version" — is agent-control/OpAMP vocabulary. Deliberately ignores the
/// failure count so the rendered message stays stable across polls.
fn cooldown_reason(reason: &SuppressionReason) -> &'static str {
    match reason {
        SuppressionReason::InCooldown { .. } => "retrying after previous failure",
        SuppressionReason::CapReached { .. } => {
            "max consecutive failures reached, retrying at the maximum backoff interval"
        }
    }
}

/// Result of a [`VersionUpdater::update`] call: whether the current process is about to restart to
/// apply a self-update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// No in-process self-update was initiated. Continue applying the remote configuration.
    /// An update may still have been requested out-of-process (k8s Helm release version bump).
    NoRestartPending,
    /// The on host self-update was initiated, the AC binary already replaced and a process restart
    /// requested. Since the restarted process will already apply the new configuration, the caller
    /// should defer sub-agent reconciliation to avoid restarting sub-agents twice.
    /// This Outcome can't ever happen for k8s since everything is handled by Helm.
    RestartPending,
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
    /// Returns [`UpdateOutcome::RestartPending`] if a self-update was initiated and the process is
    /// about to restart, [`UpdateOutcome::NoRestartPending`] if no in-process restart is pending, or
    /// an `UpdaterError` if the update fails.
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<UpdateOutcome, UpdaterError>;

    /// Re-attempts a previously-requested upgrade that has not yet succeeded, without waiting for a
    /// new desired version to be pushed. Driven by a periodic heartbeat so a transient registry
    /// outage can recover on its own.
    ///
    /// Whether an upgrade is "still pending" is implementation-defined and evaluated by the
    /// implementer, the default below is the noop fallback
    fn retry(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
}

/// A [`VersionUpdater`] that does nothing (used when version updates are not applicable).
pub struct NoOpUpdater;

impl VersionUpdater for NoOpUpdater {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<UpdateOutcome, UpdaterError> {
        Ok(UpdateOutcome::NoRestartPending)
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub VersionUpdater {}
        impl VersionUpdater for VersionUpdater {
            fn update(&self, config: &AgentControlDynamicConfig) -> Result<UpdateOutcome, UpdaterError>;
        }
    }

    impl MockVersionUpdater {
        /// Returns a mock that always returns `Ok(UpdateOutcome::NoRestartPending)` regardless of
        /// the times it is called.
        pub fn new_no_op() -> Self {
            let mut mock = Self::new();
            mock.expect_update()
                .returning(|_| Ok(UpdateOutcome::NoRestartPending));
            mock
        }
    }

    /// The OpAMP-facing message must reflect the suppression reason (not just the variant).
    #[test]
    fn update_in_cooldown_message_reflects_the_suppression_reason() {
        let in_cooldown = UpdaterError::UpdateInCooldown {
            version: "1.2.3".to_string(),
            reason: SuppressionReason::InCooldown {
                consecutive_failures: 1,
            },
        };
        assert_eq!(
            in_cooldown.to_string(),
            "upgrade to 1.2.3 suppressed: retrying after previous failure"
        );

        let cap_reached = UpdaterError::UpdateInCooldown {
            version: "1.2.3".to_string(),
            reason: SuppressionReason::CapReached {
                consecutive_failures: 5,
            },
        };
        assert_eq!(
            cap_reached.to_string(),
            "upgrade to 1.2.3 suppressed: max consecutive failures reached, retrying at the maximum backoff interval"
        );
    }
}
