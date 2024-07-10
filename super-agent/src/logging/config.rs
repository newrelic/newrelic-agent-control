use serde::Deserialize;
use std::fmt::Debug;
use std::str::FromStr;
use thiserror::Error;
use tracing::debug;
use tracing::level_filters::LevelFilter;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::Directive;
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
    #[error("logging file path not defined")]
    LogFilePathNotDefined,
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
    pub(crate) insecure_fine_grained_level: Option<String>,
    #[serde(default)]
    pub(crate) file: FileLoggingConfig,
}

pub type FileLoggerGuard = Option<WorkerGuard>;

impl LoggingConfig {
    /// Attempts to initialize the logging subscriber with the inner configuration.
    pub fn try_init(&self) -> Result<Option<WorkerGuard>, LoggingError> {
        let target = self.format.target;
        let timestamp_fmt = self.format.timestamp.0.clone();

        // Construct the file logging layer and its worker guard, only if file logging is enabled.
        // Note we can actually specify different settings for each layer (log level, format, etc),
        // hence we repeat the logic here.
        let (file_layer, guard) =
            self.file
                .clone()
                .setup()?
                .map_or(Default::default(), |(file_writer, guard)| {
                    let file_layer = tracing_subscriber::fmt::layer()
                        .with_writer(file_writer)
                        .with_ansi(false) // Disable colors for file
                        .with_target(target)
                        .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                        .fmt_fields(PrettyFields::new())
                        .with_filter(self.logging_filter());
                    (Some(file_layer), Some(guard))
                });

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(target)
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .with_filter(self.logging_filter());

        // a `Layer` wrapped in an `Option` such as the above defined `file_layer` also implements
        // the `Layer` trait. This allows individual layers to be enabled or disabled at runtime
        // while always producing a `Subscriber` of the same type.
        let subscriber = tracing_subscriber::Registry::default()
            .with(console_layer)
            .with(file_layer);

        #[cfg(feature = "tokio-console")]
        let subscriber = subscriber.with(console_subscriber::spawn());

        subscriber.try_init().map_err(|_| {
            LoggingError::TryInitError("unable to set agent global logging subscriber".to_string())
        })?;

        debug!("Logging initialized successfully");
        Ok(guard)
    }

    fn insecure_logging_filter(&self) -> Option<EnvFilter> {
        let mut env_filter = EnvFilter::builder()
            // Set all logging levels to "error". This is the highest level we can set it up.
            // Only "error" from crates will be logged.
            .with_default_directive(LevelFilter::ERROR.into())
            // Allow to remove even the default above by using a env var
            .with_env_var("INSECURE_FINE_GRAINED_LEVEL")
            // But not fail if the env var is invalid
            .from_env()
            .unwrap_or_else(|err| {
                panic!(
                    "Unparsable logging directive for environment variable `INSECURE_FINE_GRAINED_LEVEL`: {}",
                    err
                )
            });

        if let Some(s) = self.insecure_fine_grained_level.clone() {
            env_filter = env_filter.add_directive(s
                .parse::<Directive>()
                .unwrap_or_else(|err| {
                    panic!(
                        "Unparsable logging directive for config `log.insecure_fine_grained_level={}`: {}",
                        s, err
                    )
                }));
        }

        let env_filter_str = env_filter.to_string();
        let level_str = LevelFilter::ERROR.to_string();
        if env_filter_str != level_str {
            Some(env_filter)
        } else {
            None
        }
    }

    fn logging_filter(&self) -> EnvFilter {
        if let Some(insecure_logging_filter) = self.insecure_logging_filter() {
            return insecure_logging_filter;
        }

        let level = self.level.as_level().to_string().to_lowercase();

        let crate_directive = format!("newrelic_super_agent={}", level)
            .parse::<Directive>()
            // level is correctly parsed by serde at config level. If this panics, we have an issue
            // at deserialization time.
            .unwrap_or_else(|_| {
                panic!(
                    "`logging_filter` does return a unparsable directive. Panicking for level: {}",
                    level
                )
            });

        EnvFilter::builder()
            .with_default_directive(crate_directive)
            .with_env_var("LOG_LEVEL")
            .from_env_lossy()
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
