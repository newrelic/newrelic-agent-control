use log::debug;
use serde::Deserialize;
use std::fmt::Debug;
use std::str::FromStr;
use thiserror::Error;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

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
    #[serde(default)]
    pub(crate) format: LoggingFormat,
    #[serde(default)]
    pub(crate) level: LogLevel,
    #[serde(default)]
    pub(crate) file: FileLoggingConfig,
}

impl LoggingConfig {
    /// Attempts to initialize the logging subscriber with the inner configuration.
    pub fn try_init(self) -> Result<Option<WorkerGuard>, LoggingError> {
        let target = self.format.target;
        let timestamp_fmt = self.format.timestamp.0;
        let level = self.level.as_level();

        // Construct the file logging layer and its worker guard, only if file logging is enabled.
        // Note we can actually specify different settings for each layer (log level, format, etc),
        // hence we repeat the logic here.
        let (file_layer, guard) =
            self.file
                .setup()
                .map_or(Default::default(), |(file_writer, guard)| {
                    let file_layer = tracing_subscriber::fmt::layer()
                        .with_writer(file_writer)
                        .with_ansi(false) // Disable colors for file
                        .with_target(target)
                        .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                        .fmt_fields(PrettyFields::new())
                        .with_filter(
                            EnvFilter::builder()
                                .with_default_directive(level.into())
                                .with_env_var("LOG_LEVEL")
                                .from_env_lossy(),
                        );
                    (Some(file_layer), Some(guard))
                });

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(target)
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .with_filter(
                EnvFilter::builder()
                    .with_default_directive(level.into())
                    .with_env_var("LOG_LEVEL")
                    .from_env_lossy(),
            );

        // a `Layer` wrapped in an `Option` such as the above defined `file_layer` also implements
        // the `Layer` trait. This allows individual layers to be enabled or disabled at runtime
        // while always producing a `Subscriber` of the same type.
        tracing_subscriber::Registry::default()
            .with(console_layer)
            .with(file_layer)
            .try_init()
            .map_err(|_| {
                LoggingError::TryInitError(
                    "unable to set agent global logging subscriber".to_string(),
                )
            })?;

        debug!("Logging initialized successfully");
        Ok(guard)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct LogLevel(Level);

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
