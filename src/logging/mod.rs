use std::fmt::Debug;
use thiserror::Error;
use tracing_subscriber::fmt::try_init;

#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
}

pub struct Logging;

impl Logging {
    pub fn try_init() -> Result<(), LoggingError> {
        try_init().map_err(|_| {
            LoggingError::TryInitError("unable to set agent global logging subscriber".to_string())
        })
    }
}
