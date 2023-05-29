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
    StreamOutputError(#[source] SendError<OutputEvent>),

    #[error("io error")]
    IOError(#[source] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[source] nix::Error),
}

impl From<std::io::Error> for CommandError {
    fn from(value: std::io::Error) -> CommandError {
        CommandError::IOError(value)
    }
}

impl From<SendError<OutputEvent>> for CommandError {
    fn from(e: SendError<OutputEvent>) -> Self {
        CommandError::StreamOutputError(e)
    }
}

impl From<ExitStatus> for CommandError {
    fn from(value: ExitStatus) -> Self {
        CommandError::ProcessError(value)
    }
}

#[cfg(target_family = "unix")]
impl From<nix::errno::Errno> for CommandError {
    fn from(value:nix::errno::Errno) -> CommandError {
        CommandError::NixError(value)
    }
}