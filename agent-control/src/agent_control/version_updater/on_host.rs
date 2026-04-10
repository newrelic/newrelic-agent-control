use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AGENT_CONTROL_BIN_PACKAGE_ID, AgentControlDynamicConfig, Package,
};
use crate::agent_control::defaults::AGENT_CONTROL_VERSION;
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};
use crate::event::AgentControlInternalEvent;
use crate::event::channel::EventPublisher;
use crate::oci::reference_parser::ReferenceParser;
use crate::package::manager::{PackageData, PackageManager};
use core::str::FromStr;
use oci_client::Reference;
use self_replacer::SelfReplacer;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error};
use url::Url;
use wrapper_with_default::WrapperWithDefault;

pub const AGENT_CONTROL_BIN: &str = "newrelic-agent-control";

pub struct OnHostACUpdater<S, P, V>
where
    S: SelfReplacer,
    P: PackageManager,
    V: VerifyExecutor,
{
    pub ac_remote_update_enabled: bool,
    pub agent_control_internal_publisher: EventPublisher<AgentControlInternalEvent>,
    pub self_replacer: S,
    pub package_manager: Arc<P>,
    pub verify_executor: V,
    pub reference: Option<Package>,
}

impl<S, P, V> VersionUpdater for OnHostACUpdater<S, P, V>
where
    S: SelfReplacer,
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

        if new_version == AGENT_CONTROL_VERSION {
            debug!(
                "Desired agent control version {new_version} is the same as the current version, skipping update process"
            );
            return Ok(());
        }

        debug!("Starting update process for agent control version {new_version}");

        let Some(package) = &self.reference else {
            return Err(UpdaterError::UpdateFailed(
                "package reference is not specified in the updater, cannot proceed with the update process".to_string(),
            ));
        };

        let package_data = Self::get_package_data(new_version, package)?;

        let new_binary_path = self
            .package_manager
            .install(&AgentID::AgentControl, package_data)
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?
            .installation_path
            .join(AGENT_CONTROL_BIN);

        debug!(
            "Verifying new binary {} before self-replace",
            new_binary_path.to_string_lossy()
        );
        self.verify_executor
            .execute(&new_binary_path, &["verify"])
            .map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        debug!(
            "Attempting to self-replace with new binary {}",
            new_binary_path.to_string_lossy()
        );

        //TODO we should consider managing the errors that can happen in the self-replace process
        S::self_replace(&new_binary_path).map_err(|e| UpdaterError::UpdateFailed(e.to_string()))?;

        debug!(
            "Successfully updated agent control to version, stopping the agent control to allow the new version to start",
        );
        self.agent_control_internal_publisher
            .publish(AgentControlInternalEvent::StopRequested())
            .unwrap();

        Ok(())
    }
}
impl<S, P, V> OnHostACUpdater<S, P, V>
where
    P: PackageManager,
    S: SelfReplacer,
    V: VerifyExecutor,
{
    fn get_package_data(
        new_version: &String,
        package: &Package,
    ) -> Result<PackageData, UpdaterError> {
        let public_key_url = package
            .download
            .oci
            .public_key_url
            .clone()
            .map(|s| Url::parse(&s))
            .transpose()
            .map_err(|err| UpdaterError::UpdateFailed(format!("invalid public_key_url: {err}")))?;

        let string_reference = format!(
            "{}/{}{}",
            package.download.oci.registry, package.download.oci.repository, new_version
        );

        let reference = Reference::from(
            ReferenceParser::from_str(string_reference.as_str()).map_err(|err| {
                UpdaterError::UpdateFailed(format!("cannot parse reference: {err}"))
            })?,
        );

        let package_data = PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci_reference: reference,
            public_key_url,
        };
        Ok(package_data)
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
    use crate::agent_control::config::Oci;
    use crate::package::manager::tests::MockPackageManager;
    use assert_matches::assert_matches;
    use mockall::mock;
    use rstest::rstest;
    use std::time::Duration;
    use tracing_test::traced_test;

    mock! {
        pub VerifyExecutorMock {}
        impl VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), VerifyError>;
        }
    }

    /// Mock SelfReplacer for testing
    struct MockSelfReplacer;
    impl SelfReplacer for MockSelfReplacer {
        type Error = std::io::Error;

        fn self_replace(_new_bin: impl AsRef<Path>) -> Result<(), Self::Error> {
            panic!("MockSelfReplacer::self_replace should never be called in these tests")
        }
    }

    type TestUpdater = OnHostACUpdater<MockSelfReplacer, MockPackageManager, ProcessVerifyExecutor>;

    #[rstest]
    #[case("registry.io", "repo/binary:", "v1.0.0", None)]
    #[case(
        "ghcr.io",
        "org/pkg:",
        "v1.2.3",
        Some("https://keys.example.com/jwks.json")
    )]
    fn test_get_package_data_parses_reference(
        #[case] registry: &str,
        #[case] repository: &str,
        #[case] new_version: &str,
        #[case] public_key_url: Option<&str>,
    ) {
        let package = Package {
            download: crate::agent_control::config::Download {
                oci: Oci {
                    registry: registry.to_string(),
                    repository: repository.to_string(),
                    version: String::new(),
                    public_key_url: public_key_url.map(|s| s.to_string()),
                },
            },
        };

        let data = TestUpdater::get_package_data(&new_version.to_string(), &package).unwrap();
        assert_eq!(data.id, AGENT_CONTROL_BIN_PACKAGE_ID);
        assert!(data.oci_reference.to_string().contains(registry));
        assert_eq!(data.public_key_url.is_some(), public_key_url.is_some());
    }

    #[test]
    fn test_get_package_data_fails_with_invalid_url() {
        let package = Package {
            download: crate::agent_control::config::Download {
                oci: Oci {
                    registry: "registry.io".to_string(),
                    repository: "repo:".to_string(),
                    version: String::new(),
                    public_key_url: Some("not a valid url".to_string()),
                },
            },
        };

        let result = TestUpdater::get_package_data(&"v1.0.0".to_string(), &package);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid public_key_url")
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
    #[cfg_attr(unix, case("sh", vec!["-c", "printf 'some stdout'; printf 'some stderr' >&2; exit 2"]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", r#"[Console]::Write('some stdout'); [Console]::Error.Write('some stderr'); exit 2"#]))]
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
    #[cfg_attr(unix, case("sh", vec!["-c", r#"printf 'previous lines\n{"message":"pre-flight check failed"}'; exit 1"#]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", r#"Write-Output 'previous lines'; Write-Output '{"message":"pre-flight check failed"}'; exit 1"#]))]
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
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 3"]))]
    fn test_process_executor_times_out(#[case] bin: &'static str, #[case] args: Vec<&'static str>) {
        let executor = ProcessVerifyExecutor::new(Duration::from_millis(200));
        assert!(matches!(
            executor.execute(Path::new(bin), &args).unwrap_err(),
            VerifyError::Timeout(_)
        ));
    }
}
