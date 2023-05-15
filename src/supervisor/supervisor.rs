use std::result;

use thiserror::Error;

use crate::cmd::cmd::CmdError;

#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Error supervising: {0}")]
    Error(String),
}

impl From<CmdError> for SupervisorError {
    fn from(err: CmdError) -> Self {
        SupervisorError::Error(err.to_string())
    }
}

/// The result type used by this library, defaulting to [`Error`][crate::Error]
/// as the error type.
pub type Result<T> = result::Result<T, SupervisorError>;

pub trait Supervisor: Send {
    fn start(&mut self) -> Result<()>;
}