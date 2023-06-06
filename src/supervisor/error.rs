use std::{fmt::Debug, process::ExitStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("process exited with error: `{0}`")]
    ProcessExited(ExitStatus),

    #[error("io error")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[from] nix::Error),
}

impl From<ExitStatus> for ProcessError {
    fn from(value: ExitStatus) -> Self {
        ProcessError::ProcessExited(value)
    }
}
