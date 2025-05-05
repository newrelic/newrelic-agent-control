//! Uptime reporting module
//!
//! This module consists on structures to configure and operate an structure that emits
//! OpenTelemetry metrics when `tracing_opentelemetry`'s [`MetricsLayer`](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html)
//! structure is added. Otherwise, it will just emit a log message on the `TRACE` level.
//!
//! It utilizes the [`crossbeam`](https://docs.rs/crossbeam/latest/crossbeam/) crate to create a channel
//! for the uptime reporting. The channel can then be used to send messages at a specified interval.
//!
//! The uptime reporting is performed with the [`UptimeReporter`] structure and configured via the
//! [`UptimeReportConfig`] structure, which contains a boolean toggle and reporting interval.
//!
//! ```
//! # use std::time::Duration;
//! # use newrelic_agent_control::agent_control::uptime_report::{UptimeReportConfig, UptimeReporter};
//!
//! let config = UptimeReportConfig::default().with_interval(Duration::from_millis(100));
//! let reporter = UptimeReporter::from(&config);
//!
//! // Wait for the next tick
//! reporter.receiver().recv_timeout(Duration::from_millis(125)).unwrap();
//! // Report the uptime
//! assert!(reporter.report().is_ok());
//! ```

use crossbeam::channel::{Receiver, never, tick};
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::time::{Duration, Instant, SystemTime, SystemTimeError};
use tracing::trace;
use wrapper_with_default::WrapperWithDefault;

/// Default interval for uptime reporting. Set to 60 seconds.
const DEFAULT_UPTIME_REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// Default configuration for uptime reporting. Enabled by default.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct UptimeReportConfig {
    /// Whether uptime reporting is enabled or not.
    #[serde(default)]
    enabled: EnabledByDefault,
    /// Interval for uptime reporting.
    #[serde(default)]
    pub interval: UptimeReportInterval,
}

impl UptimeReportConfig {
    /// Returns whether uptime reporting is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled.0
    }

    /// Configures the interval for uptime reporting.
    pub fn with_interval(self, interval: Duration) -> Self {
        Self {
            interval: interval.into(),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
struct EnabledByDefault(bool);
impl Default for EnabledByDefault {
    fn default() -> Self {
        Self(true)
    }
}

impl From<bool> for EnabledByDefault {
    fn from(enabled: bool) -> Self {
        Self(enabled)
    }
}

/// Wrapper for the uptime report interval. This is a duration in seconds that is fixed to
/// 60 seconds via [`DEFAULT_UPTIME_REPORT_INTERVAL`].
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_UPTIME_REPORT_INTERVAL)]
pub struct UptimeReportInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// The structure actually in charge of reporting the uptime. On creation, it stores the current
/// system time.
pub struct UptimeReporter {
    start_time: SystemTime,
    ticker: Receiver<Instant>,
}

impl From<&UptimeReportConfig> for UptimeReporter {
    fn from(config: &UptimeReportConfig) -> Self {
        Self {
            start_time: SystemTime::now(),
            ticker: if config.enabled() {
                tick(config.interval.into())
            } else {
                never()
            },
        }
    }
}

impl UptimeReporter {
    /// Change the registered start time of the uptime reporter to the one passed as argument.
    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        Self { start_time, ..self }
    }

    /// Get a reference to a ticker that emits messages at the configured interval.
    /// If no interval was configured, this ticker will never trigger.
    pub fn receiver(&self) -> &Receiver<Instant> {
        &self.ticker
    }

    /// Reports the uptime by logging the elapsed time since the start time
    /// (i.e. when the reporter was created).
    ///
    /// This is expected to be a monotonic counter, and the metrics will be reported as such.
    /// It propagates failures when computing the elapsed time (see [`SystemTime::elapsed`]),
    /// which won't emit the metric, so the caller can decide how to handle it.
    pub fn report(&self) -> Result<(), SystemTimeError> {
        self.start_time
            .elapsed()
            .map(|t| trace!(monotonic_counter.uptime = t.as_secs_f64()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossbeam::select;
    use serde_json::json;
    use std::time::Duration;
    use tracing_test::traced_test;

    #[test]
    fn test_uptime_report_config() {
        let config = UptimeReportConfig::default();
        assert!(config.enabled());
        assert_eq!(config.interval.0, DEFAULT_UPTIME_REPORT_INTERVAL);
    }

    #[test]
    fn test_uptime_report_interval() {
        let interval = UptimeReportInterval(Duration::from_secs(30));
        assert_eq!(interval.0, Duration::from_secs(30));
    }

    #[test]
    fn deserialize() {
        let default_config_json = json!({});
        let expected_default_config = UptimeReportConfig::default();
        let disabled_config_json = json!({
            "enabled": false,
        });
        let expected_disabled_config = UptimeReportConfig {
            enabled: false.into(),
            ..Default::default()
        };
        let custom_duration_config_json = json!({
            "interval": "30s"
        });
        let expected_custom_duration_config = UptimeReportConfig {
            interval: UptimeReportInterval(Duration::from_secs(30)),
            enabled: true.into(),
        };

        let cases = [
            (default_config_json, expected_default_config),
            (disabled_config_json, expected_disabled_config),
            (custom_duration_config_json, expected_custom_duration_config),
        ];

        for (json, expected) in cases {
            let deserialized: UptimeReportConfig = serde_json::from_value(json).unwrap();
            assert_eq!(deserialized, expected);
        }
    }

    #[traced_test]
    #[test]
    fn test_uptime_report() {
        const EXPECTED_UPTIME_REPORTS: usize = 3;
        let config = UptimeReportConfig {
            enabled: true.into(),
            interval: UptimeReportInterval(Duration::from_millis(100)),
        };

        let reporter = UptimeReporter::from(&config);

        // Wait for three ticks
        (0..EXPECTED_UPTIME_REPORTS).for_each(|_| select! {
            recv(reporter.receiver()) -> _tick => { reporter.report().expect("Uptime report should generally not fail"); },
            default(Duration::from_millis(150)) => { panic!("Uptime report should have been triggered"); },
        });

        logs_assert(|lines| {
            // Check that the uptime was reported three times
            assert_eq!(lines.len(), EXPECTED_UPTIME_REPORTS);
            assert!(lines.iter().all(|line| {
                // lines should be at trace level
                line.contains("TRACE") &&
                // lines should emit the metrics in the format expected by `tracing_opentelemetry`
                line.contains("monotonic_counter.uptime=")
            }));
            let uptime_lines = lines
                .iter()
                // get each word of the line, find the one with the uptime and parse the value
                .map(|line| {
                    let result = line
                        .split_whitespace()
                        .filter_map(|word| {
                            word.strip_prefix("monotonic_counter.uptime=")
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                        .collect::<Vec<_>>();
                    // expecting only one element to satisfy the above filters and parse
                    result[0]
                })
                .map(Duration::from_secs_f64)
                .collect::<Vec<_>>();
            assert_eq!(uptime_lines.len(), EXPECTED_UPTIME_REPORTS);

            // Assert that the uptime log lines report uptime monotonically within a reasonable interval
            let (first, rest) = uptime_lines.split_first().unwrap();

            rest.iter()
                .try_fold(first, |previous, current| {
                    // check that the uptime is increasing by interval with a tolerance of 16ms
                    if Duration::abs_diff(*previous + Duration::from(config.interval), *current) < Duration::from_millis(16) {
                        Ok(current)
                    } else {
                        Err(format!(
                            "uptime diff exceeding 16ms toleration. Previous: {previous:?}. Current: {current:?}"
                        ))
                    }
                })
                .map(|_| ())
        });
    }
}
