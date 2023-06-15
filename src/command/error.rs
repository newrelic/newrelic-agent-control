use std::{fmt::Debug, process::ExitStatus, sync::mpsc::SendError};
use thiserror::Error;

use super::stream::Event;

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

    #[error("could not send event: `{0}`")]
    StreamSendError(#[from] SendError<Event>),

    #[error("`{0}`")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("`{0}`")]
    NixError(#[from] nix::Error),
}
