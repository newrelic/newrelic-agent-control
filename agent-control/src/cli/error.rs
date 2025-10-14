use std::process::ExitCode;

use thiserror::Error;

use crate::instrumentation::tracing::TracingError;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to initialize logs: {0}")]
    Tracing(#[from] TracingError),

    #[error("failed to start the command: {0}")]
    Precondition(String),

    #[error("{0}")]
    Command(String),
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
        }
    }
}
