//! Error type shared by the CLI commands and its mapping to process exit codes.
use std::process::ExitCode;

use crate::instrumentation::tracing::TracingError;
use thiserror::Error;

/// Errors that can occur while running a CLI command.
#[derive(Debug, Error)]
pub enum CliError {
    /// Logging/tracing initialization failed.
    #[error("failed to initialize logs: {0}")]
    Tracing(#[from] TracingError),

    /// A precondition required to start the command was not met.
    #[error("failed to start the command: {0}")]
    Precondition(String),

    /// The command itself failed while executing.
    #[error("{0}")]
    Command(String),

    /// A filesystem operation failed.
    #[error("file system error: {0}")]
    FileSystemError(String),
}

impl From<CliError> for ExitCode {
    /// Converts the error to an exit code.
    ///
    /// We comply with the [Advanced Bash Scripting Guide] and
    /// [BSD guidelines] for the exit codes.
    ///
    /// [Advanced Bash Scripting Guide]: https://tldp.org/LDP/abs/html/exitcodes.html
    /// [BSD exit codes]: https://man.freebsd.org/cgi/man.cgi?query=sysexits&manpath=FreeBSD+4.3-RELEASE
    fn from(value: CliError) -> Self {
        match value {
            CliError::Precondition(_) => Self::from(69),
            CliError::Tracing(_) => Self::from(70),
            CliError::Command(_) => Self::from(1),
            CliError::FileSystemError(_) => Self::from(1),
        }
    }
}
