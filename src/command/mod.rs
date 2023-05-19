mod error;
mod processrunner;

use std::process::ExitStatus;

use error::CommandError;

/// Trait that specifies the interface for task execution
pub trait CommandExecutor {
    type Error: std::error::Error + Send + Sync;
    type Handler: CommandHandler;

    /// The start method will execute command
    fn start(self) -> Result<Self::Handler, Self::Error>;
}

pub trait CommandHandler {
    type Error: std::error::Error + Send + Sync;

    /// The stop method will stop the command's execution
    fn stop(self) -> Result<(), Self::Error>;
}

/// Trait that specifies the interface for task execution
pub trait CommandRunner {
    type Error: std::error::Error + Send + Sync;

    /// The start method will execute command
    fn run(self) -> Result<ExitStatus, Self::Error>;
}

/// Trait that specifies the interface for a command to run
pub trait Command {
    type Proc: Process;
    fn spawn(&mut self) -> std::io::Result<Self::Proc>;
}

/// Trait that specifies the interface for a process
pub trait Process {
    fn kill(&mut self) -> std::io::Result<()>;
    fn wait(&mut self) -> std::io::Result<ExitStatus>;
}

pub use crate::command::processrunner::ProcessRunner;
