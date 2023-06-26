pub mod error;

pub mod logger;
pub mod processrunner;
pub mod shutdown;
pub use crate::command::{
    logger::StdEventReceiver, processrunner::ProcessRunner, shutdown::wait_exit_timeout,
    shutdown::wait_exit_timeout_default, shutdown::ProcessTerminator,
};
pub mod stream;

use std::{
    process::ExitStatus,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
};

use error::CommandError;
use stream::OutputEvent;

use self::stream::Event;

pub trait CommandBuilder: Send + Sync {
    type OutputType: CommandExecutor;
    fn build(&self) -> Self::OutputType;
}

pub trait TerminatorBuilder: Send + Sync {
    type OutputType: CommandTerminator;
    fn build(&self) -> Self::OutputType;
    fn with_pid(&mut self, pid: u32);
}

/// Trait that specifies the interface for a background task execution
pub trait CommandExecutor {
    type Process: CommandHandle + EventStreamer;

    /// The spawn method will execute command
    fn start(self) -> Result<Self::Process, CommandError>;
}

pub trait CommandHandle {
    type Error: std::error::Error + Send + Sync;

    fn wait(self) -> Result<ExitStatus, Self::Error>;

    fn get_pid(&self) -> u32;
}

/// Trait that specifies the interface for a blocking task execution
pub trait CommandRunner {
    type Error: std::error::Error + Send + Sync;

    /// The spawn method will execute command
    fn run(self) -> Result<ExitStatus, Self::Error>;
}

/// Trait that specifies the interface for a command terminator
pub trait CommandTerminator {
    type Error: std::error::Error + Send + Sync;

    /// The shutdown method will try to gracefully shutdown the command's execution
    fn shutdown<F>(self, func: F) -> Result<(), Self::Error>
    where
        F: FnOnce() -> bool;
}

/// This trait represents the capability of a command to stream its output.
/// As the output collection will be done in a separate thread,
/// the output will be sent through the `Sender` provided as argument.
pub trait EventStreamer {
    type Handle: CommandHandle;

    fn stream(self, snd: Sender<Event>) -> Result<Self::Handle, CommandError>;
}

/// This trait represents the capability of an Event Receiver to log its output.
/// The trait consumes itself as the logging is done in a separate thread,
/// the thread handle is returned.
pub trait EventLogger {
    fn log(self, rcv: Receiver<Event>) -> JoinHandle<()>;
}
