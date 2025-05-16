use std::process::ExitCode;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to create k8s client: {0}")]
    K8sClient(String),

    #[error("failed to create tracing: {0}")]
    Tracing(String),

    #[error("failed to apply resource: {0}")]
    ApplyResource(String),

    #[error("installation check failure: {0}")]
    InstallationCheck(String),

    #[error("deleting resource: {0}")]
    DeletingResource(String),
}

impl CliError {
    /// Converts the error to an exit code.
    ///
    /// We comply with the [Advanced Bash Scripting Guide] and
    /// [BSD guidelines] for the exit codes.
    ///
    /// [Advanced Bash Scripting Guide]: https://tldp.org/LDP/abs/html/exitcodes.html
    /// [BSD exit codes]: https://man.freebsd.org/cgi/man.cgi?query=sysexits&manpath=FreeBSD+4.3-RELEASE
    pub fn to_exit_code(&self) -> ExitCode {
        match self {
            CliError::K8sClient(_) => ExitCode::from(69),
            CliError::Tracing(_) => ExitCode::from(70),
            CliError::ApplyResource(_) | CliError::InstallationCheck(_) => ExitCode::from(1),
            CliError::DeletingResource(_) => ExitCode::from(71),
        }
    }
}
