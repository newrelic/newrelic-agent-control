use super::file_logging::FileLoggingConfig;
use super::format::LoggingFormat;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::Debug;
use std::str::FromStr;
use thiserror::Error;
use tracing::level_filters::LevelFilter;
use tracing::Level;
use tracing_subscriber::filter::{Directive, FilterExt, FilterFn};
use tracing_subscriber::layer::Filter;
use tracing_subscriber::{EnvFilter, Registry};

const LOGGING_ENABLED_CRATES: &[&str] = &["newrelic_agent_control", "opamp_client"];

const SPAN_ATTRIBUTES_MAX_LEVEL: &Level = &Level::INFO;

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum LoggingConfigError {
    #[error("invalid directive `{directive}` in `{field_name}`: {err}")]
    InvalidDirective {
        directive: String,
        field_name: String,
        err: String,
    },

    #[error("invalid logging file path: `{0}`")]
    InvalidFilePath(String),
}

/// Defines the logging configuration Agent control.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct LoggingConfig {
    /// Allows setting up custom formatting options.
    #[serde(default)]
    pub(crate) format: LoggingFormat,
    /// Defines the log level. It applies to crates defined in [LOGGING_ENABLED_CRATES] only, logs for the rest of
    /// external crates are disabled. Use `insecure_fine_grained_level` when you need logs from any crate.
    #[serde(default)]
    pub(crate) level: LogLevel,
    /// When defined, it overrides `level` and it enables logs from any crate. This cannot be considered secure since
    /// external crates log fields such as HTTP headers and leak secrets.
    #[serde(default)]
    pub(crate) insecure_fine_grained_level: Option<String>,
    /// Defines options to report logs to files
    #[serde(default)]
    pub(crate) file: FileLoggingConfig,
}

impl LoggingConfig {
    /// Returns the configured filter according to the corresponding fields. The filter will also allow
    /// any span whose level doesn't exceed [SPAN_ATTRIBUTES_MAX_LEVEL].
    pub fn filter(&self) -> Result<impl Filter<Registry>, LoggingConfigError> {
        let configured_logs_filter = self.logging_filter()?;

        let allow_spans_filter = FilterFn::new(|metadata| {
            metadata.is_span() && metadata.level() <= SPAN_ATTRIBUTES_MAX_LEVEL
        });

        let filter = allow_spans_filter.or(configured_logs_filter);

        Ok(filter)
    }

    fn logging_filter(&self) -> Result<EnvFilter, LoggingConfigError> {
        self.insecure_logging_filter()
            .unwrap_or_else(|| self.crate_logging_filter())
    }

    /// Optionally returns a filter as configured in the corresponding field (including any external crate).
    fn insecure_logging_filter(&self) -> Option<Result<EnvFilter, LoggingConfigError>> {
        self.insecure_fine_grained_level
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| {
                EnvFilter::builder()
                    .parse(s)
                    .map_err(|err| LoggingConfigError::InvalidDirective {
                        directive: s.to_string(),
                        field_name: "insecure_fine_grained_level".to_string(),
                        err: err.to_string(),
                    })
            })
    }

    /// Returns a filter for trusted crates (disables logging for any other crate).
    fn crate_logging_filter(&self) -> Result<EnvFilter, LoggingConfigError> {
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

    /// Helper to build a [Directive] corresponding to a string.
    fn logging_directive(
        directive: &str,
        field_name: &str,
    ) -> Result<Directive, LoggingConfigError> {
        directive
            .parse::<Directive>()
            .map_err(|err| LoggingConfigError::InvalidDirective {
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
    use std::fs::{self};

    use tempfile::tempdir;
    use tracing::{debug_span, error, info, info_span, warn};
    use tracing_subscriber::layer::SubscriberExt;

    use crate::instrumentation::{config::logs::file_logging::LogFilePath, tracing_layers};

    use super::*;

    #[test]
    fn test_valid_logging_filtering() {
        struct TestCase {
            name: &'static str,
            config: LoggingConfig,
            expected: &'static str,
        }

        impl TestCase {
            fn run(self) {
                let env_filter: Result<EnvFilter, LoggingConfigError> =
                    self.config.logging_filter();
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
                let filter = self.config.filter();
                let err = filter
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

    #[test]
    fn test_filtering_in_file() {
        let dir = tempdir().unwrap();
        let logs_path = dir.path().join("logs_file.log");

        // Set up warning logging level and file logging config
        let config = LoggingConfig {
            level: LogLevel(Level::WARN),
            file: FileLoggingConfig {
                enabled: true,
                path: Some(LogFilePath::try_from(logs_path.clone()).unwrap()),
            },
            ..Default::default()
        };

        // Single layer to file for testing purposes
        let (layer, file_guard) = tracing_layers::file::file(&config, dir.path().to_path_buf())
            .unwrap()
            .unwrap();

        let subscriber = tracing_subscriber::Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            let info_span = info_span!("info-span", key = "value");
            {
                let _enter = info_span.enter();
                error!("error message inside span");
                info!("info message inside span");
            }
            let debug_span = debug_span!("debug-span", key = "value");
            {
                let _enter = debug_span.enter();
                error!("error message inside debug span");
            }
            warn!("warn message outside span");
            info!("info message outside span");
        });
        drop(file_guard);

        // Check log file contents
        let paths = fs::read_dir(dir.path()).unwrap();
        // There should be one file only, but its name is not known since it includes date/time
        let path = paths.into_iter().next().unwrap().unwrap().path();
        let log_file_content = fs::read_to_string(path).unwrap();

        // WARN and ERROR messages should be included
        // Logs inside info-spans should include the corresponding fields
        assert!(log_file_content
            .contains(r#"ERROR info-span{key: "value"}: error message inside span"#));
        // Logs inside debug-spans should not include the corresponding fields
        assert!(log_file_content.contains(r"ERROR error message inside debug span"));
        // Logs outside should also be reported
        assert!(log_file_content.contains(r"WARN warn message outside span"));

        // INFO messages should not be included since we set WARN level
        assert!(!log_file_content.contains(r"info message inside span"));
        assert!(!log_file_content.contains(r"info message outside span"));
    }
}
