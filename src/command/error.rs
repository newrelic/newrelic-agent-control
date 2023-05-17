use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("process already started")]
    ProcessAlreadyStarted,

    #[error("process not started")]
    ProcessNotStarted,

    #[error("io error")]
    IOError(#[source] std::io::Error),
}

impl From<std::io::Error> for CommandError {
    fn from(value: std::io::Error) -> CommandError {
        CommandError::IOError(value)
    }
}
