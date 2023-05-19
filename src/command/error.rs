use std::{fmt::Debug, process::ExitStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("process exited with error: `{0}`")]
    ProcessError(ExitStatus),

    #[error("io error")]
    IOError(#[source] std::io::Error),
}

impl From<std::io::Error> for CommandError {
    fn from(value: std::io::Error) -> CommandError {
        CommandError::IOError(value)
    }
}

impl From<ExitStatus> for CommandError {
    fn from(value: ExitStatus) -> Self {
        CommandError::ProcessError(value)
    }
}
