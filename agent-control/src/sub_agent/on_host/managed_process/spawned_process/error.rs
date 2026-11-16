//! Errors produced while spawning or stopping an OS process.

use std::fmt::Debug;
use std::path::PathBuf;
use thiserror::Error;

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObjectError;

/// Errors produced while managing an OS process.
#[derive(Error, Debug)]
pub enum CommandError {
    /// An I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// The directory holding the process stdout/stderr log files could not be created.
    #[error("creating log directory {path}: {source}", path = path.display())]
    CreateLogDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A process stdout/stderr log file could not be opened.
    #[error("opening log file {path}: {source}", path = path.display())]
    OpenLogFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A Windows Job Object operation failed.
    #[cfg(target_family = "windows")]
    #[error("{0}")]
    JobObjectError(#[from] JobObjectError),
}
