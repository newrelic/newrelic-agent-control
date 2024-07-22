use serde::{Deserialize, Serialize, Serializer};
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
    #[error("directive `{1}` not valid `{0}`: {2}")]
    InvalidDirective(String, String, String),
    #[error("invalid logging file path: `{0}`")]
    InvalidFilePath(String),
    #[error("logging file path not defined")]
    LogFilePathNotDefined,
}

/// Defines the logging configuration for an application.
///
/// # Fields:
/// - `format`: Specifies the `LoggingFormat` the application will use for logging.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
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
        let logging_filter = self.logging_filter()?;
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
                        .with_filter(logging_filter);
                    (Some(file_layer), Some(guard))
                });

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(target)
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .with_filter(self.logging_filter()?);

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

    fn logging_filter(&self) -> Result<EnvFilter, LoggingError> {
        self.insecure_logging_filter()
            .unwrap_or_else(|| self.crate_logging_filter())
    }

    fn insecure_logging_filter(&self) -> Option<Result<EnvFilter, LoggingError>> {
        self.insecure_fine_grained_level.clone().map(|s| {
            Ok(EnvFilter::builder()
                .with_default_directive(s.parse::<Directive>().map_err(|err| {
                    LoggingError::InvalidDirective(
                        "log.insecure_fine_grained_level".to_string(),
                        s,
                        err.to_string(),
                    )
                })?)
                .parse_lossy(""))
        })
    }

    fn crate_logging_filter(&self) -> Result<EnvFilter, LoggingError> {
        let level = self.level.as_level().to_string().to_lowercase();

        Ok(EnvFilter::builder()
            .with_default_directive(LevelFilter::OFF.into())
            .parse_lossy("")
            .add_directive(
                format!("newrelic_super_agent={}", level)
                    .parse::<Directive>()
                    .map_err(|err| {
                        LoggingError::InvalidDirective(
                            "unparsable log.level".to_string(),
                            level,
                            err.to_string(),
                        )
                    })?,
            ))
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

impl Serialize for LogLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.as_level().to_string())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::logging::config::LogLevel;

    #[test]
    fn working_logging_configurations() {
        struct TestCase {
            name: &'static str,
            config: LoggingConfig,
            expected: &'static str,
        }

        impl TestCase {
            fn run(self) {
                let env_filter: Result<EnvFilter, LoggingError> = self.config.logging_filter();
                assert_eq!(
                    env_filter.unwrap().to_string(),
                    self.expected.to_string(),
                    "{}",
                    self.name
                );
            }
        }

        let test_cases = vec![
            TestCase {
                name: "everything default",
                config: Default::default(),
                expected: "newrelic_super_agent=info,off",
            },
            TestCase {
                name: "insecure fine grained overrides any logging",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("info".into()),
                    level: LogLevel(Level::INFO),
                    ..Default::default()
                },
                expected: "info",
            },
            TestCase {
                name: "parses log level from int",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_super_agent=1".into()),
                    ..Default::default()
                },
                expected: "newrelic_super_agent=error",
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn failing_logging_configurations() {
        struct TestCase {
            name: &'static str,
            config: LoggingConfig,
            expected: &'static str,
        }

        impl TestCase {
            fn run(self) {
                let env_filter: Result<EnvFilter, LoggingError> = self.config.logging_filter();
                assert_eq!(
                    env_filter.unwrap_err().to_string(),
                    self.expected.to_string(),
                    "{}",
                    self.name
                );
            }
        }

        let test_cases = vec![
            TestCase {
                name: "empty insecure fine grained",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("".into()),
                    ..Default::default()
                },
                expected: "directive `` not valid `log.insecure_fine_grained_level`: invalid filter directive",
            },
            TestCase {
                name: "invalid insecure fine grained (level as string)",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_super_agent=lolwut".into()),
                    ..Default::default()
                },
                expected: "directive `newrelic_super_agent=lolwut` not valid `log.insecure_fine_grained_level`: invalid filter directive",
            },
            TestCase {
                name: "invalid insecure fine grained (level as integer)",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_super_agent=11".into()),
                    ..Default::default()
                },
                expected: "directive `newrelic_super_agent=11` not valid `log.insecure_fine_grained_level`: invalid filter directive",
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_serialize_deserialize() {
        let l = LogLevel::default();
        assert_eq!(
            l,
            serde_yaml::from_value(serde_yaml::to_value(l.clone()).unwrap()).unwrap()
        )
    }
}
