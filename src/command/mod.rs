mod error;
mod processrunner;

use error::CommandError;

/// Trait that specifies the interface for task execution
pub(crate) trait CommandExecutor {
    type Error: std::error::Error + Send + Sync;

    /// The start method will execute command
    fn start(&mut self) -> Result<(), Self::Error>;

    /// The stop method will stop the command's execution
    fn stop(&mut self) -> Result<(), Self::Error>;
}

/// Trait that specifies the interface for a command to run
pub(crate) trait Command {
    type Proc: Process;
    fn spawn(&mut self) -> std::io::Result<Self::Proc>;
}

/// Trait that specifies the interface for a process
pub(crate) trait Process {
    fn kill(&mut self) -> std::io::Result<()>;
}
