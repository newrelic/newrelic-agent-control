use std::{fmt::Debug, process::ExitStatus, sync::mpsc::SendError};
use thiserror::Error;

use super::stream::OutputEvent;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("process exited with error: `{0}`")]
    ProcessError(ExitStatus),

    #[error("process not started")]
    ProcessNotStarted,

    #[error("command not found")]
    CommandNotFound,

    #[error("`{0}` not piped")]
    StreamPipeError(String),

    #[error("could not get output event")]
    StreamOutputError(#[from] SendError<OutputEvent>),

    #[error("io error")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[from] nix::Error),
}

impl From<ExitStatus> for CommandError {
    fn from(value: ExitStatus) -> Self {
        CommandError::ProcessError(value)
    }
}
