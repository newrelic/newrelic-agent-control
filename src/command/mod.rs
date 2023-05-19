mod error;
pub mod wrapper;

use std::process::ExitStatus;

use error::CommandError;

/// Trait that specifies the interface for a background task execution
pub trait CommandExecutor {
    type Error: std::error::Error + Send + Sync;
    type Process: CommandHandle;

    /// The spawn method will execute command
    fn start(self) -> Result<Self::Process, Self::Error>;
}

pub trait CommandHandle {
    type Error: std::error::Error + Send + Sync;

    /// The stop method will stop the command's execution
    fn stop(self) -> Result<(), Self::Error>;
}

/// Trait that specifies the interface for a blocking task execution
pub trait CommandRunner {
    type Error: std::error::Error + Send + Sync;

    /// The spawn method will execute command
    fn run(self) -> Result<ExitStatus, Self::Error>;
}
