use std::{
    fmt::Debug,
    process::ExitStatus,
    sync::{MutexGuard, PoisonError},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("process exited with error: `{0}`")]
    ProcessError(ExitStatus),

    #[error("process not started")]
    ProcessNotStarted,

    // #[error("terminate condition error")]
    // TerminateConditionError(#[source] PoisonError<MutexGuard<'static, bool>>),
    #[error("stream error")]
    StreamError,

    #[error("io error")]
    IOError(#[source] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("system error")]
    NixError(#[source] nix::Error),
}

impl From<std::io::Error> for ProcessError {
    fn from(value: std::io::Error) -> ProcessError {
        ProcessError::IOError(value)
    }
}

impl From<ExitStatus> for ProcessError {
    fn from(value: ExitStatus) -> Self {
        ProcessError::ProcessError(value)
    }
}

// impl From<PoisonError<MutexGuard<'_, bool>>> for ProcessError {
//     fn from(value: PoisonError<MutexGuard<'_, bool>>) -> Self {
//         ProcessError::TerminateConditionError(value)
//     }
// }

#[cfg(target_family = "unix")]
impl From<nix::errno::Errno> for ProcessError {
    fn from(value: nix::errno::Errno) -> ProcessError {
        ProcessError::NixError(value)
    }
}
