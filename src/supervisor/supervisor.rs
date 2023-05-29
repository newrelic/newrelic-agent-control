use std::result;

use thiserror::Error;
use crate::error::CommandError;


#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Error supervising: {0}")]
    Error(String),
}

impl From<CommandError> for SupervisorError {
    fn from(err: CommandError) -> Self {
        SupervisorError::Error(err.to_string())
    }
}

impl From<serde_json::Error> for SupervisorError {
    fn from(err: serde_json::Error) -> Self {
        SupervisorError::Error(err.to_string())
    }
}


/// The result type used by this library, defaulting to [`Error`][crate::Error]
/// as the error type.
pub type Result<T> = result::Result<T, SupervisorError>;

pub trait Supervisor: Send {
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}