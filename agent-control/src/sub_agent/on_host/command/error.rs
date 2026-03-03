use std::fmt::Debug;
use thiserror::Error;

use crate::sub_agent::on_host::command::logging::file_logger::FileLoggerError;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("{0} not piped")]
    StreamPipeError(String),

    #[error("{0}")]
    IOError(#[from] std::io::Error),

    #[error("building file logger: {0}")]
    FileLoggerError(#[from] FileLoggerError),

    #[error("{0}")]
    WinError(String),
}
