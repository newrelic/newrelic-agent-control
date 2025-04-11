//! Uptime reporting module
//!
//! This module consists on structures to configure and operate an structure that emits
//! OpenTelemetry metrics when `tracing_opentelemetry`'s [`MetricsLayer`](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html)
//! structure is added. Otherwise, it will just emit a log message on the `TRACE` level.
//!
//! It utilizes the [`crossbeam`](https://docs.rs/crossbeam/latest/crossbeam/) crate to create a channel
//! for the uptime reporting. The channel can then be used to send messages at a specified interval.
//!
//! The uptime reporting is configured via the [`UptimeReportConfig`] structure, which contains
//! a boolean flag to enable or disable the reporting, and an interval for the reporting.
//!
//! ```
//! let config = UptimeReportConfig {
//!   interval: Duration::from_millis(100).into(),
//!   ..Default::default()
//! };
//! let (uptime_reporter, uptime_report_ticker) = UptimeReporter::new_with_ticker(&config, None);
//!
//! // This will report the uptime every 100 milliseconds
//! loop {
//!     // Wait for the next tick
//!     let _ = uptime_report_ticker.recv().unwrap();
//!     // Report the uptime
//!     uptime_reporter.report().unwrap();
//! }
//! ```

use crossbeam::channel::{never, tick, Receiver};
use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, SystemTimeError};
use tracing::trace;
use wrapper_with_default::WrapperWithDefault;

/// Default interval for uptime reporting. Set to 60 seconds.
const DEFAULT_UPTIME_REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// Default configuration for uptime reporting. Enabled by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UptimeReportConfig {
    /// Whether uptime reporting is enabled or not.
    pub enabled: bool,
    /// Interval for uptime reporting.
    pub interval: UptimeReportInterval,
}

impl Default for UptimeReportConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: UptimeReportInterval::default(),
        }
    }
}

/// Wrapper for the uptime report interval. This is a duration in seconds that is fixed to
/// 60 seconds via [`DEFAULT_UPTIME_REPORT_INTERVAL`].
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_UPTIME_REPORT_INTERVAL)]
pub struct UptimeReportInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// The structure actually in charge of reporting the uptime. On creation, it stores the current
/// [`SystemTime`].
pub struct UptimeReporter {
    start_time: SystemTime,
}

impl Default for UptimeReporter {
    /// Creates a new [`UptimeReporter`] instance with the current [`SystemTime`].
    fn default() -> Self {
        Self {
            start_time: SystemTime::now(),
        }
    }
}

impl UptimeReporter {
    /// Creates a new [`UptimeReporter`] instance from an [`UptimeReportConfig`] and, optionally,
    /// a provided [`SystemTime`].
    pub fn new_with_ticker(
        config: &UptimeReportConfig,
        start_time: Option<SystemTime>,
    ) -> (Self, Receiver<Instant>) {
        let reporter = start_time.map(UptimeReporter::new).unwrap_or_default();
        let ticker = if config.enabled {
            // Report uptime for the first time
            let _ = reporter.report();
            // Deliver uptime event at the configured interval
            tick(config.interval.into())
        } else {
            never()
        };
        (reporter, ticker)
    }

    fn new(start_time: SystemTime) -> Self {
        Self { start_time }
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
    use tracing_test::traced_test;

    use super::*;
    use std::time::Duration;

    #[test]
    fn test_uptime_report_config() {
        let config = UptimeReportConfig::default();
        assert!(config.enabled);
        assert_eq!(config.interval.0, DEFAULT_UPTIME_REPORT_INTERVAL);
    }

    #[test]
    fn test_uptime_report_interval() {
        let interval = UptimeReportInterval(Duration::from_secs(30));
        assert_eq!(interval.0, Duration::from_secs(30));
    }

    #[traced_test]
    #[test]
    fn test_uptime_report() {
        let config = UptimeReportConfig {
            enabled: true,
            interval: UptimeReportInterval(Duration::from_millis(100)),
        };

        let (uptime_reporter, uptime_report_ticker) =
            UptimeReporter::new_with_ticker(&config, None);

        // Wait for three ticks
        for _ in 0..3 {
            let _ = uptime_report_ticker
                // Add a timeout so this test does not block if something is not right,
                // the expect will make us notice
                .recv_timeout(Duration::from_millis(200))
                .expect("Uptime report ticker should deliver messages at the configured interval");
            // Report the uptime
            uptime_reporter
                .report()
                .expect("Uptime report should generally not fail");
        }
        // Check that the uptime was reported three times

        logs_assert(|lines| {
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

            // Assert that the uptime log lines report uptime within a reasonable interval
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
