use crate::cli::SelfInstrumentationProviders;

use super::file_logging::FileLoggingConfig;
use super::format::LoggingFormat;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::Debug;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;
use tracing::debug;
use tracing::level_filters::LevelFilter;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_opentelemetry::MetricsLayer;
use tracing_subscriber::filter::Directive;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const LOGGING_ENABLED_CRATES: &[&str] = &["newrelic_agent_control", "opamp_client"];

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("init logging error: `{0}`")]
    TryInitError(String),
    #[error("invalid directive `{directive}` in `{field_name}`: {err}")]
    InvalidDirective {
        directive: String,
        field_name: String,
        err: String,
    },
    #[error("invalid logging file path: `{0}`")]
    InvalidFilePath(String),
}

/// Defines the logging configuration for an application.
///
/// # Fields:
/// - `format`: Specifies the `LoggingFormat` the application will use for logging.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct LoggingConfig {
    #[serde(default)]
    pub(crate) format: LoggingFormat,
    /// Defines the log level. It applies to crates defined in [LOGGING_ENABLED_CRATES] only, logs for the rest of
    /// external crates are disabled. In order to show them `insecure_fine_grained_level` needs to be set.
    #[serde(default)]
    pub(crate) level: LogLevel,
    /// When defined, it overrides `level` and it enables logs from any crate.
    #[serde(default)]
    pub(crate) insecure_fine_grained_level: Option<String>,
    #[serde(default)]
    pub(crate) file: FileLoggingConfig,
}

pub type FileLoggerGuard = Option<WorkerGuard>;

impl LoggingConfig {
    /// Attempts to initialize the logging subscriber with the inner configuration.
    pub fn try_init(
        &self,
        default_dir: PathBuf,
        self_instrumentation_providers: &SelfInstrumentationProviders,
    ) -> Result<Option<WorkerGuard>, LoggingError> {
        let target = self.format.target;
        let timestamp_fmt = self.format.timestamp.0.clone();

        // Construct the file logging layer and its worker guard, only if file logging is enabled.
        // Note we can actually specify different settings for each layer (log level, format, etc),
        // hence we repeat the logic here.
        let logging_filter = self.logging_filter()?;
        let (file_layer, guard) = self.file.clone().setup(default_dir)?.map_or(
            Default::default(),
            |(file_writer, guard)| {
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_writer(file_writer)
                    .with_ansi(false) // Disable colors for file
                    .with_target(target)
                    .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                    .fmt_fields(PrettyFields::new())
                    .with_filter(logging_filter);
                (Some(file_layer), Some(guard))
            },
        );

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(target)
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .with_filter(self.logging_filter()?);

        // Self-instrumentation layers
        let otel_traces_layer = self_instrumentation_providers.traces_provider().map(|p| {
            tracing_opentelemetry::layer()
                .with_tracer(p.tracer("agent-control-self-instrumentation"))
        });
        let otel_metrics_layer = self_instrumentation_providers
            .metrics_provider()
            .map(|p| MetricsLayer::new(p.clone()));
        let otel_logs_layer = self_instrumentation_providers
            .logs_provider()
            .map(layer::OpenTelemetryTracingBridge::new);

        // a `Layer` wrapped in an `Option` such as the above defined `file_layer` also implements
        // the `Layer` trait. This allows individual layers to be enabled or disabled at runtime
        // while always producing a `Subscriber` of the same type.
        let subscriber = tracing_subscriber::Registry::default()
            .with(console_layer)
            .with(file_layer)
            .with(otel_traces_layer)
            .with(otel_metrics_layer)
            .with(otel_logs_layer);

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
        self.insecure_fine_grained_level
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| {
                EnvFilter::builder()
                    .parse(s)
                    .map_err(|err| LoggingError::InvalidDirective {
                        directive: s.to_string(),
                        field_name: "insecure_fine_grained_level".to_string(),
                        err: err.to_string(),
                    })
            })
    }

    fn crate_logging_filter(&self) -> Result<EnvFilter, LoggingError> {
        let level = self.level.as_level().to_string().to_lowercase();

        let mut env_filter = EnvFilter::builder()
            .with_default_directive(LevelFilter::OFF.into()) // Disables logs for any crate
            .parse_lossy("");
        // Enables and sets up the log level for known crates
        for crate_name in LOGGING_ENABLED_CRATES {
            let directive = format!("{}={}", crate_name, &level);
            env_filter =
                env_filter.add_directive(Self::logging_directive(directive.as_str(), "level")?)
        }
        Ok(env_filter)
    }

    fn logging_directive(directive: &str, field_name: &str) -> Result<Directive, LoggingError> {
        directive
            .parse::<Directive>()
            .map_err(|err| LoggingError::InvalidDirective {
                directive: directive.to_string(),
                field_name: field_name.to_string(),
                err: err.to_string(),
            })
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
mod tests {
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
                expected: "newrelic_agent_control=info,opamp_client=info,off",
            },
            TestCase {
                name: "insecure fine grained overrides any logging",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("info".into()),
                    level: LogLevel(Level::DEBUG),
                    ..Default::default()
                },
                expected: "info",
            },
            TestCase {
                name: "empty insecure fine grained does not apply",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("".into()),
                    ..Default::default()
                },
                expected: "newrelic_agent_control=info,opamp_client=info,off", // default
            },
            TestCase {
                name: "several specific targets in insecure_fine_grained_level",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some(
                        "newrelic_agent_control=info,opamp_client=debug,off".into(),
                    ),
                    level: LogLevel(Level::INFO),
                    ..Default::default()
                },
                expected: "newrelic_agent_control=info,opamp_client=debug,off",
            },
            TestCase {
                name: "parses log level from int",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_agent_control=1".into()),
                    ..Default::default()
                },
                expected: "newrelic_agent_control=error",
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
                let err = env_filter
                    .err()
                    .unwrap_or_else(|| panic!("expected err got Ok - {}", self.name));
                assert_eq!(err.to_string(), self.expected.to_string(), "{}", self.name);
            }
        }

        let test_cases = vec![
            TestCase {
                name: "invalid insecure fine grained (level as string)",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_agent_control=lolwut".into()),
                    ..Default::default()
                },
                expected: "invalid directive `newrelic_agent_control=lolwut` in `insecure_fine_grained_level`: invalid filter directive",
            },
            TestCase {
                name: "invalid insecure fine grained (level as integer)",
                config: LoggingConfig {
                    insecure_fine_grained_level: Some("newrelic_agent_control=11".into()),
                    ..Default::default()
                },
                expected: "invalid directive `newrelic_agent_control=11` in `insecure_fine_grained_level`: invalid filter directive",
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
