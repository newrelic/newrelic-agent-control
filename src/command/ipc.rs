use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use libc::{SIGKILL, SIGTERM, SIGUSR1, SIGUSR2};
use thiserror::Error;

/// Trait that specifies the interface for an ipc Notifier
pub(crate) trait Notifier {
    type Error: std::error::Error + Send + Sync;

    fn notify(pid:i32, msg:Message) -> Result<(), Self::Error>;
}

#[repr(i32)]
#[cfg(target_family = "unix")]
pub enum Message {
    NotificationA(Signal) = SIGUSR1,
    NotificationB(Signal) = SIGUSR2,
    Kill(Signal) = SIGKILL,
    Term(Signal) = SIGTERM,
}

impl From<Message> for Option<Signal> {
    fn from(value: Message) -> Option<Signal> {
        match value {
            Message::NotificationA(Signal::SIGUSR1) => Some(Signal::SIGUSR1),
            Message::NotificationB(Signal::SIGUSR2) => Some(Signal::SIGUSR2),
            Message::Kill(Signal::SIGKILL) => Some(Signal::SIGKILL),
            Message::Term(Signal::SIGTERM) => Some(Signal::SIGTERM),
            _  => None
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[source] nix::Error),
}


impl From<nix::errno::Errno> for Error {
    fn from(value:nix::errno::Errno) -> Error {
        Error::NixError(value)
    }
}

#[cfg(target_family = "unix")]
pub fn notify(pid:i32, msg:Message) -> Result<(), Error> {
    let result_signal = signal::kill(Pid::from_raw(pid), msg);
    let result = match result_signal {
        Ok(signal) => Ok(signal),
        Err(error) => Err(Error::from(error)),
    };
    result
}

#[cfg(target_family = "windows")]
pub enum Message {
    String
}

