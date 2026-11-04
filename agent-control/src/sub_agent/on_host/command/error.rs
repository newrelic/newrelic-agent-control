//! Errors produced while spawning or stopping an OS command.

use std::fmt::Debug;
use thiserror::Error;

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObjectError;

/// Errors produced while managing an OS command process.
#[derive(Error, Debug)]
pub enum CommandError {
    /// An I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// The process stdout/stderr log files could not be set up.
    #[error("setting up log files: {0}")]
    LogFile(String),

    /// The process's Windows Job Object could not be created, assigned, or terminated.
    #[cfg(target_family = "windows")]
    #[error("{0}")]
    JobObjectError(#[from] JobObjectError),
}
