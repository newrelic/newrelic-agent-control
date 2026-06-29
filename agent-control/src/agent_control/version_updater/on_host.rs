//! On-host self-update [`VersionUpdater`]: downloads, verifies and self-replaces the AC binary.

pub mod verify;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AgentControlDynamicConfig, AgentControlPackage, UpgradeBackoffConfig,
};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::agent_type::runtime_config::on_host::package::rendered::{Oci, Repository, Version};
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::package::manager::{PackageData, PackageManager};
use crate::utils::backoff_gate::{BackoffGate, SuppressionReason};
use crate::utils::retry::BackoffPolicy;
use crate::utils::time::Clock;
use self_replacer::SelfReplacer;
use thiserror::Error;
use tracing::{debug, debug_span, warn};
use url::Url;
use verify::VerifyExecutor;

/// File name of the Agent Control binary on the current target.
#[cfg(target_family = "unix")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control";
/// File name of the Agent Control binary on the current target.
#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control.exe";

/// Package id used when downloading the Agent Control binary package.
pub const AGENT_CONTROL_BIN_PACKAGE_ID: &str = "agent_control_bin";

/// Error building the on-host updater from package configuration.
#[derive(Debug, Error)]
pub enum BuildError {
    /// The package config contained an invalid OCI reference.
    #[error("invalid OCI reference in package config: {0}")]
    InvalidReference(#[from] oci_client::ParseError),
}

/// On-host [`VersionUpdater`] that installs and self-replaces the Agent Control binary, with a
/// backoff gate throttling re-attempts at a failing upgrade.
pub struct OnHostACUpdater<P, V, C, R>
where
    P: PackageManager,
    V: VerifyExecutor,
    C: Clock,
    R: SelfReplacer,
{
    ac_remote_update_enabled: bool,
    agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    package_manager: P,
    verify_executor: V,
    self_replacer: R,
    repository: Repository,
    pub_key_url: Url,
    /// Throttles re-attempts at a failing upgrade so we don't hammer the registry every
    /// OpAMP poll. Keyed by target [`Version`]: a new desired version resets the cooldown.
    upgrade_gate: BackoffGate<Version, C>,
}

impl<P, V, C, R> VersionUpdater for OnHostACUpdater<P, V, C, R>
where
    P: PackageManager,
    V: VerifyExecutor,
    C: Clock,
    R: SelfReplacer,
{
    /// Only `config.version` is consumed from the dynamic config. This contract is what
    /// lets [`retry`](Self::retry) reconstruct a config from just the gate's tracked version.
    /// If `update` ever starts reading additional dynamic-config fields, that reconstruction must
    /// be revisited (or replaced with a stored snapshot of the last config) to not use defaults.
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        if !self.ac_remote_update_enabled {
            debug!("Remote update is disabled, skipping update process");
            return Ok(());
        }

        let Some(new_version) = &config.version else {
            debug!("Version is not specified in the dynamic config");
            return Ok(());
        };

        let _span = debug_span!(
            "self-update",
            previous_version = AGENT_CONTROL_VERSION,
            new_version = %new_version,
        )
        .entered();

        if new_version.to_string() == AGENT_CONTROL_VERSION {
            debug!("Desired version is the same as current, skipping update");
            self.upgrade_gate.reset();
            return Ok(());
        }

        // Cooldown gate: suppress re-attempts that are still within their backoff window, until the
        // window elapses (or the desired version changes). When it permits an attempt the gate runs
        // the upgrade and tracks success/failure itself.
        self.upgrade_gate
            .guarded(new_version, || {
                debug!("Starting update process");
                self.try_upgrade(new_version.clone())
            })
            .map_err(|e| self.suppressed_error(new_version, e))?
    }

    fn retry(&self) -> Result<(), UpdaterError> {
        // The gate's tracked key is the last version we tried to reach; if it is cleared there is
        // nothing pending, otherwise re-drive the normal `update` path for that version.
        let version = self.upgrade_gate.current_key();
        if version.is_some() {
            self.update(&AgentControlDynamicConfig {
                version,
                ..Default::default()
            })
        } else {
            Ok(())
        }
    }
}

impl<P, V, C, R> OnHostACUpdater<P, V, C, R>
where
    P: PackageManager,
    V: VerifyExecutor,
    C: Clock,
    R: SelfReplacer,
{
    /// Builds the updater from the self-update toggle, event publisher, collaborators, package
    /// source, backoff configuration and clock.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ac_remote_update_enabled: bool,
        agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
        package_manager: P,
        verify_executor: V,
        self_replacer: R,
        package: AgentControlPackage,
        backoff: UpgradeBackoffConfig,
        clock: C,
    ) -> Self {
        Self {
            ac_remote_update_enabled,
            agent_control_internal_publisher,
            package_manager,
            verify_executor,
            self_replacer,
            repository: package.download.oci.repository.clone(),
            pub_key_url: package.download.oci.public_key_url.clone(),
            // The gate owns the exponential-backoff-plus-jitter schedule (it never sleeps; it
            // records a "next attempt" instant checked across OpAMP polls).
            upgrade_gate: BackoffGate::new(BackoffPolicy::from(&backoff), clock),
        }
    }

    /// Logs and maps a gate [`SuppressionReason`] verdict for `new_version` onto the OpAMP-facing
    /// [`UpdaterError`].
    fn suppressed_error(
        &self,
        new_version: &Version,
        suppression: SuppressionReason,
    ) -> UpdaterError {
        match suppression {
            SuppressionReason::CapReached {
                consecutive_failures,
            } => warn!(
                version = %new_version,
                consecutive_failures,
                "Upgrade suppressed: max consecutive failures reached. \
                 Waiting for desired version to change before retrying.",
            ),
            SuppressionReason::InCooldown {
                consecutive_failures,
            } => debug!(
                version = %new_version,
                consecutive_failures,
                "Upgrade suppressed: in backoff cooldown window.",
            ),
        }
        UpdaterError::UpdateInCooldown {
            version: new_version.to_string(),
            reason: suppression,
        }
    }

    /// Performs a single upgrade attempt: install → verify → self-replace → request restart.
    fn try_upgrade(&self, new_version: Version) -> Result<(), UpdaterError> {
        let package_data = self.get_package_data(new_version);

        let new_binary_path = self
            .package_manager
            .install(&AgentID::AgentControl, package_data)
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("installing new Agent Control binary: {e}"))
            })?
            .installation_path
            .join(AGENT_CONTROL_BIN);

        debug!(
            binary = %new_binary_path.display(),
            "Verifying new binary before self-replace",
        );
        self.verify_executor
            .execute(&new_binary_path, &["verify"])
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("verifying new Agent Control binary: {e}"))
            })?;

        debug!("Attempting to self-replace with new binary",);

        self.self_replacer
            .self_replace(&new_binary_path)
            .map_err(|e| {
                UpdaterError::UpdateFailed(format!("self replacing Agent Control binary: {e}"))
            })?;

        debug!("Agent Control binary replaced, stopping to allow the new version to start");
        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::SelfUpdateRestartRequested())
            .map_err(|e| UpdaterError::UpdateFailed(format!("publishing stop request: {e}")))?;

        Ok(())
    }

    fn get_package_data(&self, new_version: Version) -> PackageData {
        PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci: Oci {
                repository: self.repository.clone(),
                version: new_version,
                public_key_url: Some(self.pub_key_url.clone()),
            },
            post_download_hook: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::{
        AgentControlPackage, UpgradeBaseDelay, UpgradeJitter, UpgradeMaxConsecutiveFailures,
        UpgradeMaxDelay,
    };
    use crate::event::channel::pub_sub;
    use crate::package::manager::tests::MockPackageManager;
    use crate::package::oci::package_manager::OCIPackageManagerError;
    use crate::utils::time::SystemClock;
    use mockall::mock;
    use self_replacer::BinarySelfReplacer;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    mock! {
        pub VerifyExecutorMock {}
        impl verify::VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), verify::VerifyError>;
        }
    }

    /// Test clock backed by `Arc<Mutex<Instant>>` so the test body can advance time
    /// deterministically while the updater holds the clock by value.
    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<Instant>>);

    impl FakeClock {
        fn new(initial: Instant) -> Self {
            Self(Arc::new(std::sync::Mutex::new(initial)))
        }
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    type TestUpdater<C> =
        OnHostACUpdater<MockPackageManager, MockVerifyExecutorMock, C, BinarySelfReplacer>;

    /// Replacer used by the unit tests. They all fail at `install` before reaching the
    /// self-replace step, so it never actually runs; the target is a throwaway path.
    fn unused_self_replacer() -> BinarySelfReplacer {
        BinarySelfReplacer::with_target(std::path::PathBuf::from("unused"))
    }

    fn no_jitter_backoff(base: Duration, max: Duration, max_failures: u32) -> UpgradeBackoffConfig {
        UpgradeBackoffConfig {
            base_delay: UpgradeBaseDelay::from(base),
            max_delay: UpgradeMaxDelay::from(max),
            max_consecutive_failures: UpgradeMaxConsecutiveFailures::from(max_failures),
            jitter: UpgradeJitter::from(false),
        }
    }

    fn install_failure(
        _id: &AgentID,
        _pkg: PackageData,
    ) -> Result<crate::package::manager::InstalledPackageData, OCIPackageManagerError> {
        Err(OCIPackageManagerError::Install(std::io::Error::other(
            "simulated registry failure",
        )))
    }

    fn new_test_updater_with<C: Clock>(backoff: UpgradeBackoffConfig, clock: C) -> TestUpdater<C> {
        let (publisher, _) = pub_sub();
        OnHostACUpdater::new(
            true,
            publisher,
            MockPackageManager::new(),
            MockVerifyExecutorMock::new(),
            unused_self_replacer(),
            AgentControlPackage::default(),
            backoff,
            clock,
        )
    }

    fn config_with_version(v: &str) -> AgentControlDynamicConfig {
        AgentControlDynamicConfig {
            version: Some(Version::from_str(v).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn update_is_noop_when_remote_update_disabled() {
        let (publisher, _) = pub_sub();
        let updater: TestUpdater<SystemClock> = OnHostACUpdater::new(
            false,
            publisher,
            MockPackageManager::new(),
            MockVerifyExecutorMock::new(),
            unused_self_replacer(),
            AgentControlPackage::default(),
            UpgradeBackoffConfig::default(),
            SystemClock,
        );
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_not_specified() {
        let updater = new_test_updater_with(UpgradeBackoffConfig::default(), SystemClock);
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_matches_current() {
        let updater = new_test_updater_with(UpgradeBackoffConfig::default(), SystemClock);
        let config = AgentControlDynamicConfig {
            version: Some(Version::from_str(AGENT_CONTROL_VERSION).unwrap()),
            ..Default::default()
        };
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn first_failure_then_subsequent_call_within_window_is_suppressed() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_secs(30), Duration::from_secs(600), 5),
            clock.clone(),
        );
        // Exactly one real install attempt — second attempt MUST be suppressed.
        updater
            .package_manager
            .expect_install()
            .times(1)
            .returning(install_failure);

        let cfg = config_with_version("99.99.99");

        let err = updater.update(&cfg).unwrap_err();
        assert!(matches!(err, UpdaterError::UpdateFailed(_)), "{:?}", err);

        let err = updater.update(&cfg).unwrap_err();
        assert!(matches!(
            err,
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::InCooldown { .. },
                ..
            }
        ));
    }

    #[test]
    fn cooldown_clears_after_window_elapses_and_re_attempts() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_millis(100), Duration::from_secs(10), 5),
            clock.clone(),
        );
        updater
            .package_manager
            .expect_install()
            .times(2)
            .returning(install_failure);

        let cfg = config_with_version("99.99.99");

        let _ = updater.update(&cfg).unwrap_err(); // attempt 1: fails
        let err = updater.update(&cfg).unwrap_err();
        assert!(matches!(
            err,
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::InCooldown { .. },
                ..
            }
        ));

        clock.advance(Duration::from_secs(1));

        // Attempt 2 fires (and fails again).
        let err = updater.update(&cfg).unwrap_err();
        assert!(matches!(err, UpdaterError::UpdateFailed(_)), "{:?}", err);
    }

    #[test]
    fn cap_is_reported_but_keeps_probing_after_window() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_secs(30), Duration::from_secs(30), 2),
            clock.clone(),
        );
        // The cap is a reporting threshold, not a hard stop: a probe still fires after each
        // window elapses. We expect 3 real install attempts (the 3rd happens *after* the cap).
        updater
            .package_manager
            .expect_install()
            .times(3)
            .returning(install_failure);

        let cfg = config_with_version("99.99.99");

        // Attempt 1, then suppressed within the window.
        let _ = updater.update(&cfg).unwrap_err();
        assert!(matches!(
            updater.update(&cfg).unwrap_err(),
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::InCooldown { .. },
                ..
            }
        ));

        // Attempt 2 reaches the cap; within the window it now reports CapReached.
        clock.advance(Duration::from_secs(31));
        let _ = updater.update(&cfg).unwrap_err();
        assert!(matches!(
            updater.update(&cfg).unwrap_err(),
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::CapReached { .. },
                ..
            }
        ));

        // Once the window elapses the gate probes again despite being capped.
        clock.advance(Duration::from_secs(31));
        let _ = updater.update(&cfg).unwrap_err();
    }

    #[test]
    fn version_change_resets_cooldown_and_allows_immediate_attempt() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_secs(60), Duration::from_secs(600), 5),
            clock,
        );
        // Two real install calls — bad → bad-different.
        updater
            .package_manager
            .expect_install()
            .times(2)
            .returning(install_failure);

        let _ = updater
            .update(&config_with_version("99.99.99"))
            .unwrap_err();
        // Without advancing the clock, a different version triggers a real attempt.
        let _ = updater
            .update(&config_with_version("88.88.88"))
            .unwrap_err();
    }

    #[test]
    fn reverting_to_current_version_short_circuits_even_when_capped() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_secs(60), Duration::from_secs(600), 1),
            clock.clone(),
        );
        updater
            .package_manager
            .expect_install()
            .times(1)
            .returning(install_failure);

        let _ = updater
            .update(&config_with_version("99.99.99"))
            .unwrap_err();
        // Still inside the backoff window, so the cap is reported and no second install fires.
        let err = updater
            .update(&config_with_version("99.99.99"))
            .unwrap_err();
        assert!(matches!(
            err,
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::CapReached { .. },
                ..
            }
        ));

        let cfg = AgentControlDynamicConfig {
            version: Some(Version::from_str(AGENT_CONTROL_VERSION).unwrap()),
            ..Default::default()
        };
        assert!(updater.update(&cfg).is_ok());
    }

    #[test]
    fn cooldown_error_message_is_stable_across_polls() {
        // Same variant, different failure counts: the rendered message must not change.
        let err1 = UpdaterError::UpdateInCooldown {
            version: "99.99.99".into(),
            reason: SuppressionReason::InCooldown {
                consecutive_failures: 1,
            },
        };
        let err2 = UpdaterError::UpdateInCooldown {
            version: "99.99.99".into(),
            reason: SuppressionReason::InCooldown {
                consecutive_failures: 3,
            },
        };
        assert_eq!(err1.to_string(), err2.to_string());
    }

    #[test]
    fn retry_is_noop_when_nothing_is_pending() {
        // No update() has run, so the gate has no tracked version — retry must not touch install.
        let updater = new_test_updater_with(UpgradeBackoffConfig::default(), SystemClock);
        assert!(updater.retry().is_ok());
    }

    #[test]
    fn retry_re_attempts_the_pending_version_after_the_window() {
        let clock = FakeClock::new(Instant::now());
        let mut updater = new_test_updater_with(
            no_jitter_backoff(Duration::from_secs(30), Duration::from_secs(30), 5),
            clock.clone(),
        );
        // One install from update(), one from the post-window retry() probe.
        updater
            .package_manager
            .expect_install()
            .times(2)
            .returning(install_failure);

        // A failed update() leaves the desired version pending in the gate.
        let _ = updater
            .update(&config_with_version("99.99.99"))
            .unwrap_err();

        // Within the window, retry() is suppressed and does not hit the registry.
        assert!(matches!(
            updater.retry().unwrap_err(),
            UpdaterError::UpdateInCooldown {
                reason: SuppressionReason::InCooldown { .. },
                ..
            }
        ));

        // After the window, retry() drives a fresh attempt against the pending version.
        clock.advance(Duration::from_secs(31));
        let _ = updater.retry().unwrap_err();
    }
}
