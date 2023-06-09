use log::SetLoggerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    LoggingError(#[from] SetLoggerError),
}
