use std::process::ExitStatus;
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

pub struct Unstarted;

pub struct Started;

/// The result type used by this library, defaulting to [`Error`][crate::Error]
/// as the error type.
pub type Result<T> = result::Result<T, SupervisorError>;

pub trait SupervisorExecutor: Send {
    // type Error: std::error::Error + Send + Sync;
    type Supervisor: SupervisorHandle;

    fn start(self) -> Result<(Self::Supervisor)>;
}

pub trait SupervisorHandle {
    // type Error: std::error::Error + Send + Sync;

    /// The stop method will stop the command's execution
    fn stop(self) -> Result<()>;
}

/// Trait that specifies the interface for a blocking task execution
pub trait SupervisorRunner {
    // type Error: std::error::Error + Send + Sync;

    /// The spawn method will execute command
    fn run(self) -> Result<()>;
}
