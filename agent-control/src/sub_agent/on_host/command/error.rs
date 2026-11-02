//! Errors produced while spawning or stopping an OS command.

use std::fmt::Debug;
use thiserror::Error;

use crate::sub_agent::on_host::command::logging::file_logger::FileLoggerError;
#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObjectError;

/// Errors produced while managing an OS command process.
#[derive(Error, Debug)]
pub enum CommandError {
    /// An I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// The sub-agent log files could not be opened.
    #[error("opening log files: {0}")]
    FileLoggerError(#[from] FileLoggerError),

    /// The process's Windows Job Object could not be created, assigned, or terminated.
    #[cfg(target_family = "windows")]
    #[error("{0}")]
    JobObjectError(#[from] JobObjectError),
}
