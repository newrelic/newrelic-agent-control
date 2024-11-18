use std::{fmt::Debug, process::ExitStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("`{0}` not piped")]
    StreamPipeError(String),

    #[error("`{0}`")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("`{0}`")]
    NixError(#[from] nix::Error),
}

//TODO All these interfaces are implemented just once, should we get rid of them?

/// Trait that specifies the interface for a background task execution
pub trait NotStartedCommand {
    type StartedCommand: StartedCommand;
    /// The spawn method will execute command
    fn start(self) -> Result<Self::StartedCommand, CommandError>;
}

pub trait StartedCommand {
    type StartedCommand: StartedCommand;

    fn wait(self) -> Result<ExitStatus, CommandError>;

    fn get_pid(&self) -> u32;

    /// This trait represents the capability of a command to stream its output.
    fn stream(self) -> Result<Self::StartedCommand, CommandError>;
}

pub trait SyncCommandRunner {
    /// The spawn method will execute command
    fn run(self) -> Result<ExitStatus, CommandError>;
}

/// Trait that specifies the interface for a command terminator
pub trait CommandTerminator {
    /// The shutdown method will try to gracefully shutdown the command's execution
    fn shutdown<F>(self, func: F) -> Result<(), CommandError>
    where
        F: FnOnce() -> bool;
}
