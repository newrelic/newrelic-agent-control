mod error;
pub mod ipc;
pub mod processrunner;

pub use crate::command::processrunner::ProcessRunner;
pub mod stream;

use std::{process::ExitStatus, sync::mpsc::Sender};

use error::CommandError;
use ipc::Message;
use stream::OutputEvent;

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

/// Trait that specifies the interface for an ipc Notifier
pub trait CommandNotifier {
    type Error: std::error::Error + Send + Sync;

    fn notify(&self, msg:Message) -> Result<(), Self::Error>;
}

/// Trait that specifies the interface to return the pid of a command
pub trait PidGetter {
    /// The pid method will return the pid from the executed command
    fn pid(&self) -> u32;
}

/// This trait represents the capability of a command to stream its output.
/// As the output collection will be done in a separate thread,
/// the output will be sent through the `Sender` provided as argument.
pub trait OutputStreamer {
    type Error: std::error::Error + Send + Sync;
    type Handle: CommandHandle;

    fn stream(self, snd: Sender<OutputEvent>) -> Result<Self::Handle, Self::Error>;
}
