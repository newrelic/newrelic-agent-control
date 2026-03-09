use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use thiserror::Error;
use tracing::error;
use wrapper_with_default::WrapperWithDefault;

use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_control::version_updater::updater::{UpdaterError, VersionUpdater};

// TODO adjust according to cli command expected behavior.
const DEFAULT_VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

// TODO - This cannot change between versions make sure to unify the flag with the one defined in CLI commands task.
const VERIFY_FLAG: &str = "--verify";

/// Errors that can occur while running the verification subprocess.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("{0}")]
    SubProcessError(String),

    #[error("timed out after {0:?}")]
    Timeout(Duration),

    /// Returned when the command exits with a non-zero status code, indicating
    /// that verification did not pass. The message is the human-readable
    /// explanation written by the command to stdout.
    #[error("{0}")]
    VerificationFailed(String),

    /// Returned when the command exits with a non-zero status code and its
    /// stdout cannot be parsed as [`CommandOutput`].
    #[error("unexpected failure (exit status {exit_status}) stdout={stdout} stderr={stderr}")]
    UnexpectedFailure {
        exit_status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
}

/// Output written by the verify command to stdout. (WIP)
///
/// ## Command contract
///
/// The verify command (`<binary> --verify`) is expected to behave as follows:
///
/// - **stdout**: always contains a JSON-encoded `CommandOutput` with a human-readable
///   `message`, regardless of whether verification succeeded or failed. This
///   message is suitable for logging or surfacing to operators.
/// - **exit code**: `0` signals that verification passed; any non-zero exit code
///   signals that verification failed. The message in stdout describes the reason.
///
/// TODO: Replace with the actual types defined in the CLI commands task once available.
#[derive(Debug, Deserialize)]
pub struct CommandOutput {
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
}

impl VerifyExecutor for ProcessVerifyExecutor {
    fn execute(&self, binary_path: &Path, args: &[&str]) -> Result<(), VerifyError> {
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

        // Poll until the process exits or the deadline is reached.
        let timeout: Duration = self.timeout.into();
        let deadline = Instant::now() + timeout;
        let exit_status = loop {
            match child.try_wait().map_err(|err| {
                VerifyError::SubProcessError(format!("waiting for verify process: {err}"))
            })? {
                Some(status) => break status,
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
        };

        let mut stdout_buf = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut stdout_buf);
        }

        // The exit code is the authoritative signal for success/failure.
        // On success we do not need to inspect stdout.
        if exit_status.success() {
            return Ok(());
        }

        let mut stderr_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_buf);
        }

        // On failure the command is expected to have written a structured
        // CommandOutput to stdout. If parsing fails the binary likely crashed
        // (e.g., a panic) rather than performing a controlled verification failure.
        match serde_json::from_str::<CommandOutput>(&stdout_buf) {
            Ok(output) => Err(VerifyError::VerificationFailed(output.message)),
            Err(err) => {
                error!(%err, stdout = %stdout_buf, stderr = %stderr_buf, "verification subprocess unexpectedly failed");
                Err(VerifyError::UnexpectedFailure {
                    exit_status,
                    stdout: stdout_buf,
                    stderr: stderr_buf,
                })
            }
        }
    }
}

/// On-host [`VersionUpdater`] implementation.
pub struct OnHostUpdater<E: VerifyExecutor> {
    verifier_executor: E,
}

impl<E: VerifyExecutor> OnHostUpdater<E> {
    pub fn new(verifier_executor: E) -> Self {
        Self { verifier_executor }
    }
}

impl<E: VerifyExecutor> VersionUpdater for OnHostUpdater<E> {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        // TODO: Here should downloading new binary step providing the binary path.

        let new_binary_path = PathBuf::from("/fake_path");
        self.verifier_executor
            .execute(&new_binary_path, &[VERIFY_FLAG])
            .map_err(|err| UpdaterError::UpdateFailed(format!("verifying new version: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use rstest::rstest;
    use std::time::Duration;

    mock! {
        pub VerifyExecutorMock {}
        impl VerifyExecutor for VerifyExecutorMock {
            fn execute<'a>(&self, binary_path: &Path, args: &[&'a str]) -> Result<(), VerifyError>;
        }
    }

    // ---------------------------------------------------------------------------
    // OnHostUpdater tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_update_returns_ok_when_executor_succeeds() {
        let mut executor = MockVerifyExecutorMock::new();
        executor.expect_execute().once().returning(|_, _| Ok(()));

        let updater = OnHostUpdater::new(executor);
        assert!(
            updater
                .update(&AgentControlDynamicConfig::default())
                .is_ok()
        );
    }

    #[test]
    fn test_update_returns_err_when_executor_fails() {
        let mut executor = MockVerifyExecutorMock::new();
        executor
            .expect_execute()
            .once()
            .returning(|_, _| Err(VerifyError::Timeout(DEFAULT_VERIFY_TIMEOUT)));

        let updater = OnHostUpdater::new(executor);
        let result = updater.update(&AgentControlDynamicConfig::default());
        assert!(matches!(result.unwrap_err(), UpdaterError::UpdateFailed(_)));
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

    #[rstest]
    #[cfg_attr(unix, case("false", vec![]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", "exit 1"]))]
    fn test_process_executor_unexpected_failure_on_nonzero_without_json(
        #[case] bin: &'static str,
        #[case] args: Vec<&'static str>,
    ) {
        // non-zero exit with empty stdout → no CommandOutput to parse → UnexpectedFailure
        let executor = ProcessVerifyExecutor::default();
        assert!(matches!(
            executor.execute(Path::new(bin), &args).unwrap_err(),
            VerifyError::UnexpectedFailure { .. }
        ));
    }

    #[rstest]
    #[cfg_attr(unix, case("sh", vec!["-c", r#"printf '{"message":"pre-flight check failed"}'; exit 1"#]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", r#"Write-Output '{"message":"pre-flight check failed"}'; exit 1"#]))]
    fn test_process_executor_verification_failed_on_json_stdout(
        #[case] bin: &'static str,
        #[case] args: Vec<&'static str>,
    ) {
        let executor = ProcessVerifyExecutor::default();
        let err = executor.execute(Path::new(bin), &args).unwrap_err();
        match err {
            VerifyError::VerificationFailed(msg) => assert_eq!(msg, "pre-flight check failed"),
            other => panic!("expected VerificationFailed, got {other:?}"),
        }
    }

    #[rstest]
    #[cfg_attr(unix, case("sleep", vec!["1"]))]
    #[cfg_attr(windows, case("powershell", vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 1"]))]
    fn test_process_executor_times_out(#[case] bin: &'static str, #[case] args: Vec<&'static str>) {
        let executor = ProcessVerifyExecutor::new(Duration::from_millis(200));
        assert!(matches!(
            executor.execute(Path::new(bin), &args).unwrap_err(),
            VerifyError::Timeout(_)
        ));
    }
}
