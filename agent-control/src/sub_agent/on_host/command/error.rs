//! Errors produced while spawning, streaming, or stopping an OS command.

use std::fmt::Debug;
use thiserror::Error;

use crate::sub_agent::on_host::command::logging::file_logger::FileLoggerError;

/// Errors produced while managing an OS command process.
#[derive(Error, Debug)]
pub enum CommandError {
    /// A standard stream (stdout/stderr) could not be piped.
    #[error("{0} not piped")]
    StreamPipeError(String),

    /// An I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// The file logger could not be built.
    #[error("building file logger: {0}")]
    FileLoggerError(#[from] FileLoggerError),

    /// A Windows-specific API error occurred.
    #[error("{0}")]
    WinError(String),
}
