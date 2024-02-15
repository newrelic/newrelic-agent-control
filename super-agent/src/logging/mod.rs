use serde::Deserialize;
use std::fmt::Debug;
use thiserror::Error;
use tracing::metadata::LevelFilter;
use tracing::Level;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::EnvFilter;

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
}

/// Defines the logging configuration for an application.
///
/// # Fields:
/// - `format`: Specifies the `LoggingFormat` the application will use for logging.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct LoggingConfig {
    pub(crate) format: LoggingFormat,
}

/// Represents a custom time stamp format for logging.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub(crate) struct TimestampFormat(pub(crate) String);

/// Provides a default `TimestampFormat`. The default format is based on
/// [chrono strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html#fn7)
///
/// # Returns:
/// A new `TimestampFormat` instance with the format set as "%Y-%m-%dT%H:%M:%S".
impl Default for TimestampFormat {
    fn default() -> Self {
        Self("%Y-%m-%dT%H:%M:%S".to_string())
    }
}

/// Defines the format to be used for logging, including target and timestamp.
///
/// # Fields:
/// - `target`: A bool that indicates whether or not the target of the trace event will be included in the formatted output.
/// - `timestamp`: Specifies a `TimestampFormat` the application will use for logging timestamps.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct LoggingFormat {
    pub(crate) target: bool,
    #[serde(default)]
    pub(crate) timestamp: TimestampFormat,
}

impl LoggingConfig {
    /// Attempts to initialize the logging subscriber with the inner configuration.
    pub fn try_init(self) -> Result<(), LoggingError> {
        tracing_subscriber::fmt()
            .with_target(self.format.target)
            .with_max_level(Level::INFO)
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .with_env_var("LOG_LEVEL")
                    .from_env_lossy(),
            )
            .with_timer(ChronoLocal::new(self.format.timestamp.0))
            .fmt_fields(PrettyFields::new())
            .try_init()
            .map_err(|_| {
                LoggingError::TryInitError(
                    "unable to set agent global logging subscriber".to_string(),
                )
            })
    }
}
