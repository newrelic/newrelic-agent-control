use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{AgentControlDynamicConfig, AgentControlPackage};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::oci::reference_parser::ReferenceParser;
use crate::package::manager::{PackageData, PackageManager};
use oci_client::Reference;
use self_replacer::{BinarySelfReplacer, SelfReplacer};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, debug_span, error};
use url::Url;
use wrapper_with_default::WrapperWithDefault;

#[cfg(target_family = "unix")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control";
#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control.exe";

pub const AGENT_CONTROL_BIN_PACKAGE_ID: &str = "agent_control_bin";

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid OCI reference in package config: {0}")]
    InvalidReference(#[from] oci_client::ParseError),
}
pub struct OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
    ac_remote_update_enabled: bool,
    agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    package_manager: Arc<P>,
    verify_executor: V,
    base_reference: Reference,
    pub_key_url: Url,
}

impl<P, V> VersionUpdater for OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
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

        if new_version == AGENT_CONTROL_VERSION {
            debug!("Desired version is the same as current, skipping update");
            return Ok(());
        }

        debug!("Starting update process");

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

        BinarySelfReplacer::self_replace(&new_binary_path).map_err(|e| {
            UpdaterError::UpdateFailed(format!("self replacing Agent Control binary: {e}"))
        })?;

        debug!("Agent Control binary replaced, stopping to allow the new version to start");
        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::StopRequested())
            .map_err(|e| UpdaterError::UpdateFailed(format!("publishing stop request: {e}")))?;

        Ok(())
    }
}
impl<P, V> OnHostACUpdater<P, V>
where
    P: PackageManager,
    V: VerifyExecutor,
{
    pub fn try_new(
        ac_remote_update_enabled: bool,
        agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
        package_manager: Arc<P>,
        verify_executor: V,
        package: AgentControlPackage,
    ) -> Result<Self, BuildError> {
        let base_reference = Reference::from(ReferenceParser::from_str(
            format!(
                "{}/{}",
                package.download.oci.registry, package.download.oci.repository
            )
            .as_str(),
        )?);
        Ok(Self {
            ac_remote_update_enabled,
            agent_control_internal_publisher,
            package_manager,
            verify_executor,
            base_reference,
            pub_key_url: package.download.oci.public_key_url,
        })
    }

    fn get_package_data(&self, new_version: &str) -> PackageData {
        let reference = Reference::with_tag(
            self.base_reference.registry().to_string(),
            self.base_reference.repository().to_string(),
            new_version.to_string(),
        );
        PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci_reference: reference,
            public_key_url: Some(self.pub_key_url.clone()),
            preinstall_script_path: None,
            postinstall_script_path: None,
        }
    }
}

// Configuration and OpAMP connectivy checks take up 8 seconds in total.
// Setting the default timeout to 20 seconds gives room for the checks to complete while
// avoiding excessively long waits in case of hangs or crashes.
const DEFAULT_VERIFY_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Errors that can occur while running the verification subprocess.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("dry-run check of new version failed due to subprocess error: {0}")]
    SubProcessError(String),

    #[error("dry-run check of new version timed out after {0:?}")]
    Timeout(Duration),

    /// Returned when the command exits with a non-zero status code, indicating
    /// that verification did not pass. The message is the human-readable
    /// explanation written by the command to stdout.
    #[error("dry-run check of new version failed with: {0}")]
    VerificationFailed(String),

    /// Returned when the command exits with a non-zero status code and its
    /// stdout cannot be parsed as [`CommandResult`].
    #[error("dry-run check of new version failed unexpectedly")]
    UnexpectedFailure,
}

/// Output written by the verify command to stdout.
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub message: String,
}

/// Abstraction for executing the verification command. For testing purposes.
pub trait VerifyExecutor {
    fn execute(&self, binary_path: &Path, args: &[&str]) -> Result<(), VerifyError>;
}

/// Timeout for the verification subprocess, defaulting to [`DEFAULT_VERIFY_TIMEOUT`].
#[derive(Debug, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_VERIFY_TIMEOUT)]
pub struct VerifyTimeout(Duration);

/// Real implementation of [`VerifyExecutor`] that spawns an OS subprocess.
///
/// ## Command contract
///
/// The verify command (`<binary> verify`) is expected to behave as follows:
///
/// - **stdout**: always contains a JSON-encoded `CommandResult` with a human-readable
///   `message`, regardless of whether verification succeeded or failed. This
///   message is suitable for logging or surfacing to operators.
/// - **exit code**: `0` signals that verification passed; any non-zero exit code
///   signals that verification failed. The message in stdout describes the reason.
#[derive(Debug, Default)]
pub struct ProcessVerifyExecutor {
    timeout: VerifyTimeout,
}

impl ProcessVerifyExecutor {
    pub fn new(timeout: impl Into<VerifyTimeout>) -> Self {
        Self {
            timeout: timeout.into(),
        }
    }

    fn wait_for_exit(
        &self,
        child: &mut Child,
        timeout: Duration,
    ) -> Result<ExitStatus, VerifyError> {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait().map_err(|err| {
                VerifyError::SubProcessError(format!("waiting for verify process: {err}"))
            })? {
                Some(status) => return Ok(status),
                None => {
                    if Instant::now() >= deadline {
                        child.kill().map_err(|err| {
                            VerifyError::SubProcessError(format!("killing verify process: {err}"))
                        })?;
                        return Err(VerifyError::Timeout(timeout));
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }
}

impl VerifyExecutor for ProcessVerifyExecutor {
    fn execute(&self, binary_path: &Path, args: &[&str]) -> Result<(), VerifyError> {
        debug!(binary = %binary_path.display(), ?args, "Spawning verify subprocess");
        // The child inherits the parent environment by default; no explicit
        // .envs() call needed.
        let mut child = Command::new(binary_path)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                VerifyError::SubProcessError(format!("spawning verify process: {err}"))
            })?;

        debug!("Verify subprocess started");

        let exit_status = self.wait_for_exit(&mut child, self.timeout.into())?;

        debug!(%exit_status, "Verify subprocess exited");

        // The exit code is the authoritative signal for success/failure.
        // On success we do not need to inspect stdout.
        if exit_status.success() {
            return Ok(());
        }

        let mut stdout_buf = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut stdout_buf);
        }

        let mut stderr_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_buf);
        }

        // On failure the command is expected to have written a structured
        // CommandOutput to stdout. If parsing fails the binary likely crashed
        // (e.g., a panic) rather than performing a controlled verification failure.
        let output_to_parse = stdout_buf
            .lines()
            .filter_map(|line| serde_json::from_str::<CommandResult>(line).ok())
            .next_back();

        match output_to_parse {
            Some(output) => Err(VerifyError::VerificationFailed(output.message)),
            None => {
                // exit code of -1 indicates that the process was terminated by a signal (Unix)
                error!(stdout = %stdout_buf, stderr = %stderr_buf, exit_code = exit_status.code().unwrap_or(-1), "Verification subprocess failed and output couldn't be parsed");
                Err(VerifyError::UnexpectedFailure)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::{AgentControlPackage, Download, Oci};
    use crate::event::channel::pub_sub;
    use crate::package::manager::tests::MockPackageManager;
    use assert_matches::assert_matches;
    use mockall::mock;
    use rstest::rstest;
    use std::sync::Arc;
    use std::time::Duration;
    use tracing_test::traced_test;
    use url::Url;

    mock! {
        pub VerifyExecutorMock {}
        impl VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), VerifyError>;
        }
    }

    type TestUpdater = OnHostACUpdater<MockPackageManager, MockVerifyExecutorMock>;

    fn new_test_updater(ac_remote_update_enabled: bool) -> TestUpdater {
        let (publisher, _) = pub_sub();
        OnHostACUpdater::try_new(
            ac_remote_update_enabled,
            publisher,
            Arc::new(MockPackageManager::new()),
            MockVerifyExecutorMock::new(),
            AgentControlPackage::default(),
        )
        .unwrap()
    }

    #[test]
    fn update_is_noop_when_remote_update_disabled() {
        let updater = new_test_updater(false);
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_not_specified() {
        let updater = new_test_updater(true);
        let config = AgentControlDynamicConfig::default();
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn update_is_noop_when_version_matches_current() {
        let updater = new_test_updater(true);
        let config = AgentControlDynamicConfig {
            version: Some(AGENT_CONTROL_VERSION.to_string()),
            ..Default::default()
        };
        assert!(updater.update(&config).is_ok());
    }

    #[test]
    fn try_new_fails_with_invalid_oci_reference() {
        let (publisher, _) = pub_sub();
        let package = AgentControlPackage {
            download: Download {
                oci: Oci {
                    registry: "invalid registry with spaces".to_string(),
                    repository: "repo".to_string(),
                    public_key_url: Url::parse("https://newrelic.com/keys").unwrap(),
                },
            },
        };
        assert_matches!(
            TestUpdater::try_new(
                true,
                publisher,
                Arc::new(MockPackageManager::new()),
                MockVerifyExecutorMock::new(),
                package
            )
            .err()
            .unwrap(),
            BuildError::InvalidReference(_)
        );
    }

    // ---------------------------------------------------------------------------
    // ProcessVerifyExecutor tests — real subprocesses
    // ---------------------------------------------------------------------------

    #[rstest]
    #[cfg_attr(unix, case("true", vec![]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", "exit 0"]))]
    fn test_process_executor_succeeds_on_zero_exit(
        #[case] bin: &'static str,
        #[case] args: Vec<&'static str>,
    ) {
        let executor = ProcessVerifyExecutor::default();
        assert!(executor.execute(Path::new(bin), &args).is_ok());
    }

    #[traced_test]
    #[rstest]
    #[cfg_attr(unix, case("sh", vec!["-c", "printf 'some stdout'; printf 'some stderr' >&2; exit 2"]
    ))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", r#"[Console]::Write('some stdout'); [Console]::Error.Write('some stderr'); exit 2"#]
    ))]
    fn test_process_executor_unexpected_failure_contains_stdout_stderr_and_exit_status(
        #[case] bin: &'static str,
        #[case] args: Vec<&'static str>,
    ) {
        let executor = ProcessVerifyExecutor::default();
        let err = executor.execute(Path::new(bin), &args).unwrap_err();
        assert_matches!(err, VerifyError::UnexpectedFailure);

        assert!(logs_contain(
            "Verification subprocess failed and output couldn't be parsed stdout=some stdout stderr=some stderr exit_code=2"
        ));
    }

    #[rstest]
    #[cfg_attr(unix, case("sh", vec!["-c", r#"printf 'previous lines\n{"message":"pre-flight check failed"}'; exit 1"#]
    ))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", r#"Write-Output 'previous lines'; Write-Output '{"message":"pre-flight check failed"}'; exit 1"#]
    ))]
    fn test_process_executor_verification_failed_on_json_stdout(
        #[case] bin: &'static str,
        #[case] args: Vec<&'static str>,
    ) {
        let executor = ProcessVerifyExecutor::default();
        let err = executor.execute(Path::new(bin), &args).unwrap_err();
        assert_matches!(err, VerifyError::VerificationFailed(msg) if msg == "pre-flight check failed");
    }

    #[rstest]
    #[cfg_attr(unix, case("sleep", vec!["3"]))]
    #[cfg_attr(
        windows,
        case("powershell", vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 3"])
    )]
    fn test_process_executor_times_out(#[case] bin: &'static str, #[case] args: Vec<&'static str>) {
        let executor = ProcessVerifyExecutor::new(Duration::from_millis(200));
        assert!(matches!(
            executor.execute(Path::new(bin), &args).unwrap_err(),
            VerifyError::Timeout(_)
        ));
    }
}
