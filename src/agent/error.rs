use log::SetLoggerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("logging error: `{0}`")]
    LoggingError(#[from] SetLoggerError),
}
