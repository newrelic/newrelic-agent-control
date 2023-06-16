use std::fmt::Debug;
use thiserror::Error;
use tracing::metadata::LevelFilter;
use tracing::Level;
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
            .with_max_level(Level::INFO)
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .fmt_fields(PrettyFields::new())
            .try_init()
            .map_err(|_| {
                LoggingError::TryInitError(
                    "unable to set agent global logging subscriber".to_string(),
                )
            })
    }
}
