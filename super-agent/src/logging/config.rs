use serde::Deserialize;
use std::fmt::Debug;
use std::str::FromStr;
use thiserror::Error;
use tracing::metadata::LevelFilter;
use tracing::Level;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::EnvFilter;

use super::file_logging::FileLoggingConfig;
use super::format::LoggingFormat;

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
    #[error("invalid logging file path: `{0}`")]
    InvalidFilePath(String),
}

/// Defines the logging configuration for an application.
///
/// # Fields:
/// - `format`: Specifies the `LoggingFormat` the application will use for logging.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct LoggingConfig {
    pub(crate) format: LoggingFormat,
    level: LogLevel,
    #[serde(default)]
    file: FileLoggingConfig,
}

impl LoggingConfig {
    /// Attempts to initialize the logging subscriber with the inner configuration.
    pub fn try_init(self) -> Result<(), LoggingError> {
        let target = self.format.target;
        let timestamp_fmt = self.format.timestamp.0;
        let level = self.level.as_level();

        let (file_layer, guard) = match self.file.setup() {
            None => (None, None),
            Some((file_writer, guard)) => (Some(_), Some(guard)),
        };

        tracing_subscriber::fmt()
            .with_target(target)
            .with_max_level(level)
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive(level.into())
                    .with_env_var("LOG_LEVEL")
                    .from_env_lossy(),
            )
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .try_init()
            .map_err(|_| {
                LoggingError::TryInitError(
                    "unable to set agent global logging subscriber".to_string(),
                )
            })
    }
}

#[derive(Debug, PartialEq, Clone)]
struct LogLevel(Level);

impl LogLevel {
    fn as_level(&self) -> Level {
        self.0
    }
}

impl Default for LogLevel {
    fn default() -> Self {
        Self(Level::INFO)
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value_str = String::deserialize(deserializer)?;
        Level::from_str(&value_str)
            .map(LogLevel)
            .map_err(serde::de::Error::custom)
    }
}
