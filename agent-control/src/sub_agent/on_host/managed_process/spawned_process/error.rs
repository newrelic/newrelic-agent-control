//! Errors produced while spawning or stopping an OS process.

use std::fmt::Debug;
use thiserror::Error;

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObjectError;

/// Errors produced while managing an OS process.
#[derive(Error, Debug)]
pub enum CommandError {
    /// An I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// The process stdout/stderr log files could not be set up.
    #[error("setting up log files: {0}")]
    LogFile(String),

    /// A Windows Job Object operation failed.
    #[cfg(target_family = "windows")]
    #[error("{0}")]
    JobObjectError(#[from] JobObjectError),
}
