use std::fmt::Debug;
use thiserror::Error;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::EnvFilter;

#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
}

pub struct Logging;

impl Logging {
    pub fn try_init() -> Result<(), LoggingError> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .fmt_fields(PrettyFields::new())
            .try_init()
            .map_err(|_| {
                LoggingError::TryInitError(
                    "unable to set agent global logging subscriber".to_string(),
                )
            })
    }
}
