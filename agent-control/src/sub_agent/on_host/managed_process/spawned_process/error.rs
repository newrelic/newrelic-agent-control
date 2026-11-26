//! Errors produced while spawning or stopping an OS process.

use std::fmt::Debug;
use std::path::PathBuf;
use thiserror::Error;

use crate::sub_agent::on_host::process_group_tracker::ProcessGroupTrackerError;

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

    /// A process tracker operation failed.
    #[error("{0}")]
    Tracker(ProcessGroupTrackerError),
}
