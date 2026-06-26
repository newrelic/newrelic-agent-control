//! On-host self-update [`VersionUpdater`]: downloads, verifies and self-replaces the AC binary.

pub mod verify;

use std::sync::{Arc, Mutex, MutexGuard};

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AgentControlDynamicConfig, AgentControlPackage, UpgradeBackoffConfig,
};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::updater::{UpdateOutcome, UpdaterError, VersionUpdater};
use crate::agent_type::runtime_config::on_host::package::rendered::{Oci, Repository, Version};
use crate::event::AgentControlInternalEvent;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
use crate::package::manager::{PackageData, PackageManager};
use crate::utils::backoff_gate::{BackoffGate, SuppressionReason};
use crate::utils::retry::BackoffPolicy;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use crate::utils::time::Clock;
use crossbeam::channel::select;
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

const SELF_UPDATE_WORKER_THREAD_NAME: &str = "self-update-worker";

/// Error building the on-host updater from package configuration.
#[derive(Debug, Error)]
pub enum BuildError {
    /// The package config contained an invalid OCI reference.
    #[error("invalid OCI reference in package config: {0}")]
    InvalidReference(#[from] oci_client::ParseError),
}

/// A unit of work handed to the self-update worker: install/verify/self-replace this version.
#[derive(Debug, Clone, PartialEq)]
struct Job {
    version: Version,
}

/// Whether the worker currently has an upgrade attempt in flight. Used for single-flight.
#[derive(Debug, PartialEq, Eq)]
enum WorkerState {
    Idle,
    InFlight,
}

/// Result of a single staged upgrade attempt run by the worker.
#[derive(Debug)]
enum AttemptOutcome {
    /// Binary installed, verified and self-replaced; a restart was requested.
    Completed,
    /// A stop was requested mid-attempt (shutdown) before the irreversible self-replace.
    Aborted,
    /// The attempt failed; carries the rendered error.
    Failed(String),
}

/// Main-thread-owned decision state for self-update. Decides whether to dispatch an attempt
/// (consulting the [`BackoffGate`] cooldown) and records the worker's result. It never performs
/// the blocking work itself — that runs on the worker thread, outside this struct's lock.
struct UpdateController<C: Clock> {
    /// Exponential-backoff-plus-jitter cooldown keyed by target [`Version`]. A new desired
    /// version resets the cooldown.
    gate: BackoffGate<Version, C>,
    state: WorkerState,
    in_flight: Option<Version>,
    /// Latest desired version requested while an attempt was in flight. Dispatched when the
    /// in-flight attempt finishes so rapid-fire configs converge to the newest target (F-10).
    desired: Option<Version>,
    job_tx: EventPublisher<Job>,
}

impl<C: Clock> UpdateController<C> {
    fn new(gate: BackoffGate<Version, C>, job_tx: EventPublisher<Job>) -> Self {
        Self {
            gate,
            state: WorkerState::Idle,
            in_flight: None,
            desired: None,
            job_tx,
        }
    }

    /// Decides whether to dispatch an upgrade to `version`. Never blocks.
    ///
    /// - In flight, same version: no-op (F-18 — don't enqueue a duplicate download).
    /// - In flight, different version: recorded as the latest desired target (F-10) and dispatched
    ///   when the in-flight attempt finishes; the newest request wins.
    /// - Idle: consult the cooldown gate; if it permits, mark in-flight and enqueue the job.
    fn dispatch(&mut self, version: Version) -> Result<UpdateOutcome, SuppressionReason> {
        match self.state {
            WorkerState::InFlight => {
                // Single-flight: only remember a *different* version (a re-push of the in-flight
                // one is a no-op). The newest desired version overwrites any earlier one.
                if self.in_flight.as_ref() != Some(&version) {
                    self.desired = Some(version);
                }
                Ok(UpdateOutcome::Dispatched)
            }
            WorkerState::Idle => match self.gate.check(&version) {
                Some(reason) => Err(reason),
                None => {
                    self.state = WorkerState::InFlight;
                    self.in_flight = Some(version.clone());
                    // Best-effort: if the worker is gone the upgrade just won't run; the gate
                    // still tracks the version so retry()/the next poll can re-drive it.
                    let _ = self.job_tx.publish(Job { version });
                    Ok(UpdateOutcome::Dispatched)
                }
            },
        }
    }

    /// Records the worker's result for `version` and returns the controller to `Idle`. On a failed
    /// attempt, if a newer version was requested meanwhile it is dispatched immediately (F-10).
    fn record_result(&mut self, version: &Version, outcome: &AttemptOutcome) {
        self.state = WorkerState::Idle;
        self.in_flight = None;
        match outcome {
            // Success: a restart is imminent; nothing to converge.
            AttemptOutcome::Completed => {
                self.gate.reset();
                self.desired = None;
            }
            // Shutting down: leave the gate untouched (no spurious failure) and don't converge.
            AttemptOutcome::Aborted => {
                self.desired = None;
            }
            AttemptOutcome::Failed(_) => {
                self.gate.record_failure(version);
                // F-10 convergence: dispatch the newest desired version now (a new gate key resets
                // the cooldown) instead of waiting for the next poll. A re-request of the
                // just-failed version stays under cooldown and is left to the retry heartbeat.
                if let Some(next) = self.desired.take()
                    && &next != version
                {
                    let _ = self.dispatch(next);
                }
            }
        }
    }

    fn reset_gate(&self) {
        self.gate.reset();
    }

    fn is_in_flight(&self) -> bool {
        matches!(self.state, WorkerState::InFlight)
    }

    /// The version the gate is currently tracking (the last one we tried to reach), if any.
    fn pending_version(&self) -> Option<Version> {
        self.gate.current_key()
    }
}

/// Performs the staged, blocking upgrade work. Owned by the worker thread.
struct UpgradeExecutor<P: PackageManager, V: VerifyExecutor, R: SelfReplacer> {
    package_manager: P,
    verify_executor: V,
    self_replacer: R,
    repository: Repository,
    pub_key_url: Url,
}

impl<P: PackageManager, V: VerifyExecutor, R: SelfReplacer> UpgradeExecutor<P, V, R> {
    fn package_data(&self, version: Version) -> PackageData {
        PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci: Oci {
                repository: self.repository.clone(),
                version,
                public_key_url: Some(self.pub_key_url.clone()),
            },
            post_download_hook: None,
        }
    }

    /// Runs a single upgrade attempt: install → verify → self-replace. The `cancel` signal is
    /// checked between the irreversible stages so a shutdown never proceeds to self-replace.
    /// The install and verify calls themselves are not interruptible, but the point of no return
    /// (self-replace) is always gated behind a cancellation check.
    fn run(&self, version: Version, cancel: &EventConsumer<CancellationMessage>) -> AttemptOutcome {
        let new_binary_path = match self
            .package_manager
            .install(&AgentID::AgentControl, self.package_data(version))
        {
            Ok(installed) => installed.installation_path.join(AGENT_CONTROL_BIN),
            Err(e) => {
                return AttemptOutcome::Failed(format!("installing new Agent Control binary: {e}"));
            }
        };

        if cancel.is_cancelled() {
            return AttemptOutcome::Aborted;
        }

        debug!(binary = %new_binary_path.display(), "Verifying new binary before self-replace");
        if let Err(e) = self.verify_executor.execute(&new_binary_path, &["verify"]) {
            return AttemptOutcome::Failed(format!("verifying new Agent Control binary: {e}"));
        }

        if cancel.is_cancelled() {
            return AttemptOutcome::Aborted;
        }

        debug!("Attempting to self-replace with new binary");
        if let Err(e) = self.self_replacer.self_replace(&new_binary_path) {
            return AttemptOutcome::Failed(format!("self replacing Agent Control binary: {e}"));
        }

        AttemptOutcome::Completed
    }
}

/// The worker thread body: waits for jobs, runs each staged upgrade, publishes the result back to
/// the event loop, and records it on the controller. Exits on a stop signal (or a cancel mid-job).
fn run_worker<P, V, R, C>(
    job_rx: EventConsumer<Job>,
    stop: EventConsumer<CancellationMessage>,
    executor: UpgradeExecutor<P, V, R>,
    publisher: EventPublisher<AgentControlInternalEvent>,
    controller: Arc<Mutex<UpdateController<C>>>,
) where
    P: PackageManager,
    V: VerifyExecutor,
    R: SelfReplacer,
    C: Clock,
{
    loop {
        select! {
            recv(job_rx.as_ref()) -> job => {
                let Ok(Job { version }) = job else {
                    return; // channel closed
                };
                let _span = debug_span!("self-update", new_version = %version).entered();

                let outcome = executor.run(version.clone(), &stop);
                match &outcome {
                    AttemptOutcome::Completed => {
                        debug!("Agent Control binary replaced, requesting restart");
                        let _ = publisher
                            .publish(AgentControlInternalEvent::SelfUpdateRestartRequested());
                    }
                    AttemptOutcome::Failed(error_message) => {
                        warn!(error = %error_message, "Self-update attempt failed");
                        let _ = publisher.publish(AgentControlInternalEvent::SelfUpdateFailed {
                            error_message: error_message.clone(),
                        });
                    }
                    AttemptOutcome::Aborted => {
                        debug!("Self-update attempt aborted due to shutdown");
                    }
                }

                lock_controller(&controller).record_result(&version, &outcome);

                if matches!(outcome, AttemptOutcome::Aborted) {
                    return; // shutdown in progress
                }
            }
            recv(stop.as_ref()) -> _ => return,
        }
    }
}

/// Locks the controller, recovering from a poisoned mutex (the controller holds only small state;
/// a panic while holding it must not deadlock the updater).
fn lock_controller<C: Clock>(
    controller: &Arc<Mutex<UpdateController<C>>>,
) -> MutexGuard<'_, UpdateController<C>> {
    controller
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Logs and maps a gate [`SuppressionReason`] verdict for `new_version` onto the OpAMP-facing
/// [`UpdaterError`].
fn suppressed_error(new_version: &Version, suppression: SuppressionReason) -> UpdaterError {
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

/// On-host self-updater. `update()` is non-blocking: it decides (under the controller lock) whether
/// to dispatch an upgrade and enqueues the blocking install/verify/self-replace work onto a
/// background worker thread, so the Agent Control event loop is never frozen during an upgrade.
/// Success is reported via [`AgentControlInternalEvent::SelfUpdateRestartRequested`] and failure via
/// [`AgentControlInternalEvent::SelfUpdateFailed`].
pub struct OnHostACUpdater<C: Clock> {
    ac_remote_update_enabled: bool,
    controller: Arc<Mutex<UpdateController<C>>>,
    worker: Option<StartedThreadContext>,
}

impl<C: Clock + Send + 'static> OnHostACUpdater<C> {
    /// Builds the updater from the self-update toggle, event publisher, collaborators, package
    /// source, backoff configuration and clock. The blocking install/verify/self-replace work is
    /// handed to a background worker thread started here.
    #[allow(clippy::too_many_arguments)]
    pub fn new<P, V, R>(
        ac_remote_update_enabled: bool,
        agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
        package_manager: P,
        verify_executor: V,
        self_replacer: R,
        package: AgentControlPackage,
        backoff: UpgradeBackoffConfig,
        clock: C,
    ) -> Self
    where
        P: PackageManager + Send + 'static,
        V: VerifyExecutor + Send + 'static,
        R: SelfReplacer + Send + 'static,
    {
        let (job_tx, job_rx) = pub_sub::<Job>();
        // The gate owns the exponential-backoff-plus-jitter schedule (it never sleeps; it records
        // a "next attempt" instant checked across OpAMP polls).
        let gate = BackoffGate::new(BackoffPolicy::from(&backoff), clock);
        let controller = Arc::new(Mutex::new(UpdateController::new(gate, job_tx)));

        let executor = UpgradeExecutor {
            package_manager,
            verify_executor,
            self_replacer,
            repository: package.download.oci.repository.clone(),
            pub_key_url: package.download.oci.public_key_url.clone(),
        };

        let worker_controller = controller.clone();
        let worker = NotStartedThreadContext::new(SELF_UPDATE_WORKER_THREAD_NAME, move |stop| {
            run_worker(
                job_rx,
                stop,
                executor,
                agent_control_internal_publisher,
                worker_controller,
            )
        })
        .start();

        Self {
            ac_remote_update_enabled,
            controller,
            worker: Some(worker),
        }
    }
}

impl<C: Clock> VersionUpdater for OnHostACUpdater<C> {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<UpdateOutcome, UpdaterError> {
        if !self.ac_remote_update_enabled {
            debug!("Remote update is disabled, skipping update process");
            return Ok(UpdateOutcome::NoOp);
        }

        let Some(new_version) = &config.version else {
            debug!("Version is not specified in the dynamic config");
            return Ok(UpdateOutcome::NoOp);
        };

        let _span = debug_span!(
            "self-update",
            previous_version = AGENT_CONTROL_VERSION,
            new_version = %new_version,
        )
        .entered();

        if new_version.to_string() == AGENT_CONTROL_VERSION {
            debug!("Desired version is the same as current, skipping update");
            lock_controller(&self.controller).reset_gate();
            return Ok(UpdateOutcome::NoOp);
        }

        // Decide + enqueue under the lock; the blocking work runs on the worker thread.
        lock_controller(&self.controller)
            .dispatch(new_version.clone())
            .map_err(|reason| suppressed_error(new_version, reason))
    }

    fn retry(&self) -> Result<(), UpdaterError> {
        let mut controller = lock_controller(&self.controller);
        // A worker is already running an attempt; nothing to re-drive.
        if controller.is_in_flight() {
            return Ok(());
        }
        // The gate's tracked key is the last version we tried to reach; if it is cleared there is
        // nothing pending, otherwise re-drive the dispatch path for that version.
        let Some(version) = controller.pending_version() else {
            return Ok(());
        };
        controller
            .dispatch(version.clone())
            .map(|_| ())
            .map_err(|reason| suppressed_error(&version, reason))
    }
}

impl<C: Clock> Drop for OnHostACUpdater<C> {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Signal stop and join. The worker checks cancellation before the irreversible
            // self-replace, so a stop during an attempt aborts cleanly without replacing.
            let _ = worker.stop_blocking();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::manager::InstalledPackageData;
    use crate::package::manager::tests::MockPackageManager;
    use crate::package::oci::package_manager::OCIPackageManagerError;
    use crate::utils::time::SystemClock;
    use verify::VerifyError;

    use mockall::mock;
    use self_replacer::BinaryReplacer;
    use std::num::NonZeroUsize;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    mock! {
        pub VerifyExecutorMock {}
        impl verify::VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), VerifyError>;
        }
    }

    /// Replacer used by the unit tests. They all abort or fail before reaching the self-replace
    /// step, so it never actually runs; the target is a throwaway path.
    fn unused_self_replacer() -> BinaryReplacer {
        BinaryReplacer::with_target(PathBuf::from("unused"))
    }

    /// Test clock backed by `Arc<Mutex<Instant>>` so a test can advance time deterministically.
    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<Instant>>);
    impl FakeClock {
        fn new(initial: Instant) -> Self {
            Self(Arc::new(Mutex::new(initial)))
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

    fn ver(v: &str) -> Version {
        Version::from_str(v).unwrap()
    }

    fn installed() -> InstalledPackageData {
        InstalledPackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            // A path we never actually read in these tests (self-replace is never reached).
            installation_path: PathBuf::from("/nonexistent/agent-control-test"),
        }
    }

    // ---- UpdateController (decision logic) ----

    fn controller(
        base: Duration,
        max: Duration,
        max_attempts: usize,
        clock: FakeClock,
    ) -> (UpdateController<FakeClock>, EventConsumer<Job>) {
        let (job_tx, job_rx) = pub_sub::<Job>();
        let policy = BackoffPolicy {
            max_attempts: NonZeroUsize::new(max_attempts).unwrap(),
            base_delay: base,
            max_delay: max,
            jitter: false,
        };
        (
            UpdateController::new(BackoffGate::new(policy, clock), job_tx),
            job_rx,
        )
    }

    fn next_job(job_rx: &EventConsumer<Job>) -> Option<Version> {
        job_rx.as_ref().try_recv().ok().map(|j| j.version)
    }

    #[test]
    fn dispatch_when_idle_enqueues_and_marks_in_flight() {
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));
        let v = ver("99.99.99");

        assert_eq!(ctrl.dispatch(v.clone()), Ok(UpdateOutcome::Dispatched));
        assert!(ctrl.is_in_flight());
        assert_eq!(next_job(&job_rx), Some(v));
    }

    #[test]
    fn dispatch_same_version_in_flight_is_deduped() {
        // F-18: a re-push of the version already being installed must not enqueue a second job.
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));
        let v = ver("99.99.99");

        ctrl.dispatch(v.clone()).unwrap();
        assert_eq!(next_job(&job_rx), Some(v.clone()));
        assert_eq!(ctrl.dispatch(v), Ok(UpdateOutcome::Dispatched));
        assert_eq!(next_job(&job_rx), None);
    }

    #[test]
    fn dispatch_different_version_in_flight_is_recorded_as_desired() {
        // F-10: a different version mid-flight isn't started now, but is remembered as `desired`.
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));

        ctrl.dispatch(ver("99.99.99")).unwrap();
        assert_eq!(next_job(&job_rx), Some(ver("99.99.99")));
        assert_eq!(ctrl.dispatch(ver("88.88.88")), Ok(UpdateOutcome::Dispatched));
        assert_eq!(next_job(&job_rx), None); // not started while the first is in flight
        assert_eq!(ctrl.in_flight, Some(ver("99.99.99")));
        assert_eq!(ctrl.desired, Some(ver("88.88.88")));
    }

    #[test]
    fn converges_to_desired_after_failure() {
        // F-10: when the in-flight attempt fails, the newer desired version is dispatched.
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));

        ctrl.dispatch(ver("99.99.99")).unwrap();
        assert_eq!(next_job(&job_rx), Some(ver("99.99.99")));
        ctrl.dispatch(ver("88.88.88")).unwrap(); // recorded as desired

        ctrl.record_result(&ver("99.99.99"), &AttemptOutcome::Failed("boom".into()));

        // The desired version is dispatched immediately (a new gate key resets the cooldown).
        assert_eq!(next_job(&job_rx), Some(ver("88.88.88")));
        assert_eq!(ctrl.in_flight, Some(ver("88.88.88")));
        assert_eq!(ctrl.desired, None);
    }

    #[test]
    fn latest_desired_version_wins() {
        // F-10: the newest request overwrites an earlier desired version.
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));

        ctrl.dispatch(ver("99.99.99")).unwrap();
        let _ = next_job(&job_rx);
        ctrl.dispatch(ver("88.88.88")).unwrap();
        ctrl.dispatch(ver("77.77.77")).unwrap(); // overwrites 88.88.88
        assert_eq!(ctrl.desired, Some(ver("77.77.77")));

        ctrl.record_result(&ver("99.99.99"), &AttemptOutcome::Failed("boom".into()));
        assert_eq!(next_job(&job_rx), Some(ver("77.77.77")));
    }

    #[test]
    fn success_does_not_converge() {
        // On success a restart is imminent, so a desired version is dropped (not dispatched).
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));

        ctrl.dispatch(ver("99.99.99")).unwrap();
        let _ = next_job(&job_rx);
        ctrl.dispatch(ver("88.88.88")).unwrap(); // desired

        ctrl.record_result(&ver("99.99.99"), &AttemptOutcome::Completed);
        assert_eq!(next_job(&job_rx), None);
        assert_eq!(ctrl.desired, None);
        assert!(!ctrl.is_in_flight());
    }

    #[test]
    fn failure_suppressed_within_window_then_reattempts() {
        let clock = FakeClock::new(Instant::now());
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, clock.clone());
        let v = ver("99.99.99");

        ctrl.dispatch(v.clone()).unwrap();
        assert_eq!(next_job(&job_rx), Some(v.clone()));
        ctrl.record_result(&v, &AttemptOutcome::Failed("boom".into()));

        // Within the cooldown window → suppressed, no job.
        assert!(matches!(
            ctrl.dispatch(v.clone()),
            Err(SuppressionReason::InCooldown { .. })
        ));
        assert_eq!(next_job(&job_rx), None);

        // After the window → re-dispatched.
        clock.advance(Duration::from_secs(31));
        assert_eq!(ctrl.dispatch(v.clone()), Ok(UpdateOutcome::Dispatched));
        assert_eq!(next_job(&job_rx), Some(v));
    }

    #[test]
    fn success_resets_cooldown() {
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));
        let v = ver("99.99.99");

        ctrl.dispatch(v.clone()).unwrap();
        assert_eq!(next_job(&job_rx), Some(v.clone()));
        ctrl.record_result(&v, &AttemptOutcome::Completed);

        assert!(!ctrl.is_in_flight());
        assert_eq!(ctrl.dispatch(v.clone()), Ok(UpdateOutcome::Dispatched));
        assert_eq!(next_job(&job_rx), Some(v));
    }

    #[test]
    fn aborted_leaves_cooldown_untouched() {
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));
        let v = ver("99.99.99");

        ctrl.dispatch(v.clone()).unwrap();
        assert_eq!(next_job(&job_rx), Some(v.clone()));
        ctrl.record_result(&v, &AttemptOutcome::Aborted);

        // No cooldown recorded → immediate re-dispatch is allowed.
        assert_eq!(ctrl.dispatch(v.clone()), Ok(UpdateOutcome::Dispatched));
        assert_eq!(next_job(&job_rx), Some(v));
    }

    #[test]
    fn pending_version_tracks_gate_key() {
        let (mut ctrl, job_rx) =
            controller(Duration::from_secs(30), Duration::from_secs(600), 5, FakeClock::new(Instant::now()));
        let v = ver("99.99.99");

        assert_eq!(ctrl.pending_version(), None);
        ctrl.dispatch(v.clone()).unwrap();
        let _ = next_job(&job_rx);
        assert_eq!(ctrl.pending_version(), Some(v.clone()));
        ctrl.record_result(&v, &AttemptOutcome::Completed);
        assert_eq!(ctrl.pending_version(), None);
    }

    // ---- UpgradeExecutor (staged work + cancellation) ----

    fn executor(
        pm: MockPackageManager,
        ve: MockVerifyExecutorMock,
    ) -> UpgradeExecutor<MockPackageManager, MockVerifyExecutorMock, BinaryReplacer> {
        let package = AgentControlPackage::default();
        UpgradeExecutor {
            package_manager: pm,
            verify_executor: ve,
            self_replacer: unused_self_replacer(),
            repository: package.download.oci.repository.clone(),
            pub_key_url: package.download.oci.public_key_url.clone(),
        }
    }

    #[test]
    fn run_returns_failed_when_install_fails() {
        let mut pm = MockPackageManager::new();
        pm.expect_install()
            .times(1)
            .returning(|_, _| Err(OCIPackageManagerError::Install(std::io::Error::other("boom"))));
        let mut ve = MockVerifyExecutorMock::new();
        ve.expect_execute().never();

        let (_tx, stop_rx) = pub_sub::<CancellationMessage>();
        assert!(matches!(
            executor(pm, ve).run(ver("99.99.99"), &stop_rx),
            AttemptOutcome::Failed(_)
        ));
    }

    #[test]
    fn run_aborts_before_verify_when_cancelled() {
        let mut pm = MockPackageManager::new();
        pm.expect_install().times(1).returning(|_, _| Ok(installed()));
        let mut ve = MockVerifyExecutorMock::new();
        ve.expect_execute().never(); // must NOT verify after a cancel

        let (stop_tx, stop_rx) = pub_sub::<CancellationMessage>();
        stop_tx.publish(()).unwrap(); // cancelled before the attempt runs
        assert!(matches!(
            executor(pm, ve).run(ver("99.99.99"), &stop_rx),
            AttemptOutcome::Aborted
        ));
    }

    #[test]
    fn run_returns_failed_when_verify_fails() {
        let mut pm = MockPackageManager::new();
        pm.expect_install().times(1).returning(|_, _| Ok(installed()));
        let mut ve = MockVerifyExecutorMock::new();
        ve.expect_execute()
            .times(1)
            .returning(|_, _| Err(VerifyError::UnexpectedFailure));

        let (_tx, stop_rx) = pub_sub::<CancellationMessage>();
        assert!(matches!(
            executor(pm, ve).run(ver("99.99.99"), &stop_rx),
            AttemptOutcome::Failed(_)
        ));
    }

    #[test]
    fn run_aborts_after_verify_when_cancelled_before_replace() {
        // verify succeeds but a stop arrives during it; the attempt must abort BEFORE self-replace.
        let mut pm = MockPackageManager::new();
        pm.expect_install().times(1).returning(|_, _| Ok(installed()));

        let (stop_tx, stop_rx) = pub_sub::<CancellationMessage>();
        let stop_tx_in_verify = stop_tx.clone();
        let mut ve = MockVerifyExecutorMock::new();
        ve.expect_execute().times(1).returning(move |_, _| {
            let _ = stop_tx_in_verify.publish(());
            Ok(())
        });

        assert!(matches!(
            executor(pm, ve).run(ver("99.99.99"), &stop_rx),
            AttemptOutcome::Aborted
        ));
    }

    // ---- OnHostACUpdater early-return (no-op) behavior ----

    fn new_test_updater(enabled: bool) -> OnHostACUpdater<SystemClock> {
        let (publisher, _consumer) = pub_sub();
        OnHostACUpdater::new(
            enabled,
            publisher,
            MockPackageManager::new(),
            MockVerifyExecutorMock::new(),
            unused_self_replacer(),
            AgentControlPackage::default(),
            UpgradeBackoffConfig::default(),
            SystemClock,
        )
    }

    #[test]
    fn update_is_noop_when_remote_update_disabled() {
        let updater = new_test_updater(false);
        assert_eq!(
            updater.update(&AgentControlDynamicConfig::default()).unwrap(),
            UpdateOutcome::NoOp
        );
    }

    #[test]
    fn update_is_noop_when_version_not_specified() {
        let updater = new_test_updater(true);
        assert_eq!(
            updater.update(&AgentControlDynamicConfig::default()).unwrap(),
            UpdateOutcome::NoOp
        );
    }

    #[test]
    fn update_is_noop_when_version_matches_current() {
        let updater = new_test_updater(true);
        let cfg = AgentControlDynamicConfig {
            version: Some(Version::from_str(AGENT_CONTROL_VERSION).unwrap()),
            ..Default::default()
        };
        assert_eq!(updater.update(&cfg).unwrap(), UpdateOutcome::NoOp);
    }

    #[test]
    fn retry_is_noop_when_nothing_pending() {
        let updater = new_test_updater(true);
        assert!(updater.retry().is_ok());
    }
}