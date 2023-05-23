use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use libc::{SIGKILL, SIGTERM, SIGUSR1, SIGUSR2};
use thiserror::Error;

#[repr(i32)]
#[cfg(target_family = "unix")]
pub enum Message {
    NotificationA = SIGUSR1,
    NotificationB = SIGUSR2,
    Kill = SIGKILL,
    Term = SIGTERM,
}

#[cfg(target_family = "unix")]
impl From<Message> for Option<Signal> {
    fn from(value: Message) -> Option<Signal> {
        match value {
            Message::NotificationA => Some(Signal::SIGUSR1),
            Message::NotificationB => Some(Signal::SIGUSR2),
            Message::Kill => Some(Signal::SIGKILL),
            Message::Term => Some(Signal::SIGTERM),
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[source] nix::Error),
}

#[cfg(target_family = "unix")]
impl From<nix::errno::Errno> for Error {
    fn from(value:nix::errno::Errno) -> Error {
        Error::NixError(value)
    }
}

#[cfg(target_family = "unix")]
pub(crate) fn notify(pid:u32, msg:Message) -> Result<(), Error> {
    let result_signal = signal::kill(Pid::from_raw(pid as i32), msg);
    let result = match result_signal {
        Ok(signal) => Ok(signal),
        Err(error) => Err(Error::from(error)),
    };
    result
}

#[cfg(not(target_family = "unix"))]
pub enum Message {}

#[cfg(not(target_family = "unix"))]
pub(crate) fn notify(pid:u32, msg:Message) -> Result<(), Error> {
    Ok(())
}
