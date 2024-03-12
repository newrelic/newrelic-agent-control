use std::{fmt::Debug, process::ExitStatus};

use thiserror::Error;

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

    #[error("`{0}`")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("`{0}`")]
    NixError(#[from] nix::Error),
}

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

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::mock;
    #[cfg(target_family = "windows")]
    use std::os::windows::process::ExitStatusExt;

    mock! {
            pub StartedCommandMock {}

            impl StartedCommand for StartedCommandMock {
                type StartedCommand = MockStartedCommandMock;

                fn wait(self) -> Result<ExitStatus, CommandError>;
                fn get_pid(&self) -> u32;
                fn stream(self) -> Result<MockStartedCommandMock, CommandError>;
        }
    }

    mock! {
        pub NotStartedCommandRunnerMock {}

        impl NotStartedCommand for NotStartedCommandRunnerMock {
            type StartedCommand = MockStartedCommandMock;

            fn start(self) -> Result<MockStartedCommandMock, CommandError>;
        }
    }
}
