use duration_str::deserialize_duration;
use opentelemetry_sdk::logs;
use opentelemetry_sdk::trace;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug, time::Duration};
use url::Url;

use crate::{http::config::ProxyConfig, reporter::UptimeReporterInterval};

/// Default timeout for HTTP client.
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default interval for exporting metrics.
const DEFAULT_METRICS_EXPORT_INTERVAL: Duration = Duration::from_secs(60);
/// Default maximum batch size [trace::BatchSpanProcessor] for details.
const DEFAULT_BATCH_MAX_SIZE: usize = 512;
/// Default scheduled delay [trace::BatchSpanProcessor] for details.
const DEFAULT_BATCH_SCHEDULED_DELAY: Duration = Duration::from_secs(30);
/// Default insecure_level filter.
const DEFAULT_FILTER: &str = "newrelic_agent_control=debug,opamp_client=debug,off";

/// Traces suffix for the OpenTelemetry endpoint
const TRACES_SUFFIX: &str = "/v1/traces";
/// Metrics suffix for the OpenTelemetry endpoint
const METRICS_SUFFIX: &str = "/v1/metrics";
/// Metrics suffix for the OpenTelemetry endpoint
const LOGS_SUFFIX: &str = "/v1/logs";

/// Represents the OpenTelemetry configuration
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct OtelConfig {
    /// Metrics configuration
    #[serde(default)]
    pub(crate) metrics: MetricsConfig,
    /// Traces configuration
    #[serde(default)]
    pub(crate) traces: TracesConfig,
    /// Logs configuration
    #[serde(default)]
    pub(crate) logs: LogsConfig,
    /// Filter metrics, tracer, and logs. By default [DEFAULT_FILTER] is used. This is marked as
    /// insecure because sensitive data could be sent if some crates are not filtered like the http client.
    #[serde(default = "default_insecure_level")]
    pub(crate) insecure_level: String,
    /// OpenTelemetry HTTP base endpoint to report instrumentation, to send each instrumentation
    /// type, the corresponding suffix will be added [TRACES_SUFFIX], [METRICS_SUFFIX], [LOGS_SUFFIX].
    pub(crate) endpoint: Url,
    /// Headers to include in every request to the OpenTelemetry endpoint
    #[serde(default)]
    pub(crate) headers: HashMap<String, String>,
    /// Client timeout
    pub(crate) client_timeout: ClientTimeout,
    /// Client proxy configuration. It is supposed to take global proxy configuration, that's why it is skipped in
    /// serde serialization and deserialization.
    #[serde(skip)]
    pub(crate) proxy: ProxyConfig,

    #[serde(default)]
    pub(crate) uptime_reporter: UptimeReporterConfig,
}

fn default_insecure_level() -> String {
    DEFAULT_FILTER.to_string()
}

impl OtelConfig {
    /// Returns a new configuration including proxy config
    pub fn with_proxy_config(self, proxy: ProxyConfig) -> Self {
        Self { proxy, ..self }
    }

    /// Returns the otel endpoint to report traces to.
    pub(crate) fn traces_endpoint(&self) -> String {
        self.target_endpoint(TRACES_SUFFIX)
    }

    /// Returns the otel endpoint to report metrics to.
    pub(crate) fn metrics_endpoint(&self) -> String {
        self.target_endpoint(METRICS_SUFFIX)
    }

    /// Returns the otel endpoint to report logs to.
    pub(crate) fn logs_endpoint(&self) -> String {
        self.target_endpoint(LOGS_SUFFIX)
    }

    /// Helper to get the endpoint for each data type
    ///
    /// # Panics
    /// - If the suffix is not a valid suffix to append to the url
    fn target_endpoint(&self, suffix: &str) -> String {
        self.endpoint
            .join(suffix)
            .unwrap_or_else(|err| {
                panic!("this is a bug: invalid suffix '{suffix}' for otel endpoint: {err}")
            })
            .to_string()
    }
}

/// Defines the configuration setting to report metrics to OpenTelemetry
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct MetricsConfig {
    /// Indicates if metrics are enabled or not
    pub(crate) enabled: bool,
    /// Sets up the interval to report metrics. They are reported periodically according to it.
    pub(crate) interval: MetricsExportInterval,
}

/// Defines the configuration settings to report traces to OpenTelemetry
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct TracesConfig {
    /// Indicates if traces are enabled or not
    pub(crate) enabled: bool,
    /// Traces are reported in batches, this field defines the batch configuration.
    pub(crate) batch_config: BatchConfig,
}

/// Defines the configuration settings to report logs to OpenTelemetry
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct LogsConfig {
    /// Indicates if logs are enabled or not
    pub(crate) enabled: bool,
    /// Traces are reported in batches, this field defines the batch configuration.
    pub(crate) batch_config: BatchConfig,
}

/// Type to represent a client timeout. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl From<Duration> for ClientTimeout {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<ClientTimeout> for Duration {
    fn from(value: ClientTimeout) -> Self {
        value.0
    }
}

impl Default for ClientTimeout {
    fn default() -> Self {
        Self(DEFAULT_CLIENT_TIMEOUT)
    }
}

/// Type to represent the metrics export interval. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MetricsExportInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl From<Duration> for MetricsExportInterval {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<MetricsExportInterval> for Duration {
    fn from(value: MetricsExportInterval) -> Self {
        value.0
    }
}

impl Default for MetricsExportInterval {
    fn default() -> Self {
        Self(DEFAULT_METRICS_EXPORT_INTERVAL)
    }
}

/// Holds the batch configuration to send traces/logs telemetry data through OpenTelemetry.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub(crate) struct BatchConfig {
    scheduled_delay: Duration,
    max_size: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            scheduled_delay: DEFAULT_BATCH_SCHEDULED_DELAY,
            max_size: DEFAULT_BATCH_MAX_SIZE,
        }
    }
}

impl From<&BatchConfig> for trace::BatchConfig {
    fn from(value: &BatchConfig) -> Self {
        trace::BatchConfigBuilder::default()
            .with_max_export_batch_size(value.max_size)
            .with_scheduled_delay(value.scheduled_delay)
            .build()
    }
}
impl From<&BatchConfig> for logs::BatchConfig {
    fn from(value: &BatchConfig) -> Self {
        logs::BatchConfigBuilder::default()
            .with_max_export_batch_size(value.max_size)
            .with_scheduled_delay(value.scheduled_delay)
            .build()
    }
}

/// Configuration for [`UptimeReporter`](crate::reporter::uptime::UptimeReporter).
#[derive(Debug, Deserialize, Default, Serialize, PartialEq, Clone)]
pub(crate) struct UptimeReporterConfig {
    /// Toggle to enable/disable the uptime reporter.
    #[serde(default)]
    enabled: UptimeReporterEnabled,
    /// Interval to report the uptime. Default is 60 seconds.
    #[serde(default)]
    pub(crate) interval: UptimeReporterInterval,
}

impl UptimeReporterConfig {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.0
    }
}

/// Wraps the uptime reporter toggle so it's enabled by default in the absence of a config.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
struct UptimeReporterEnabled(bool);

impl Default for UptimeReporterEnabled {
    fn default() -> Self {
        Self(true)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::*;

    impl Default for OtelConfig {
        fn default() -> Self {
            Self::default_with_endpoint("https://fake")
        }
    }
    impl OtelConfig {
        fn default_with_endpoint(endpoint: &str) -> Self {
            Self {
                metrics: Default::default(),
                traces: Default::default(),
                logs: Default::default(),
                insecure_level: default_insecure_level(),
                endpoint: endpoint.parse().unwrap(),
                headers: Default::default(),
                client_timeout: Default::default(),
                proxy: Default::default(),
                uptime_reporter: UptimeReporterConfig::default(),
            }
        }
    }
    #[test]
    fn test_endpoints() {
        let config = OtelConfig::default_with_endpoint("https://some.endpoint:4318");
        assert_eq!(
            config.traces_endpoint(),
            "https://some.endpoint:4318/v1/traces".to_string()
        );
        assert_eq!(
            config.metrics_endpoint(),
            "https://some.endpoint:4318/v1/metrics".to_string()
        );
        assert_eq!(
            config.logs_endpoint(),
            "https://some.endpoint:4318/v1/logs".to_string()
        );
    }

    #[test]
    fn test_defaults() {
        let config = OtelConfig::default_with_endpoint("https://some.endpoint:4318");
        let default_batch_config = BatchConfig::default();

        assert_eq!(default_batch_config.max_size, DEFAULT_BATCH_MAX_SIZE);
        assert_eq!(
            default_batch_config.scheduled_delay,
            DEFAULT_BATCH_SCHEDULED_DELAY
        );

        assert_eq!(config.traces.batch_config, default_batch_config);
        assert!(!config.traces.enabled);

        assert!(!config.metrics.enabled);
        assert_eq!(
            Duration::from(config.metrics.interval),
            DEFAULT_METRICS_EXPORT_INTERVAL
        );

        assert!(!config.logs.enabled);

        assert_eq!(
            Duration::from(config.client_timeout),
            DEFAULT_CLIENT_TIMEOUT
        );
    }

    #[test]
    fn uptime_reporter_config() {
        let config = UptimeReporterConfig::default();
        assert!(config.enabled());
        assert_eq!(config.interval, UptimeReporterInterval::default());
    }

    #[test]
    fn uptime_reporter_config_deserialize_missing_values() {
        let all_empty = json!({});

        let config: UptimeReporterConfig = serde_json::from_value(all_empty).unwrap();
        assert_eq!(config, UptimeReporterConfig::default());

        let enable_only = json!( {
            "enabled": false,
        });

        let config: UptimeReporterConfig = serde_json::from_value(enable_only).unwrap();
        assert!(!config.enabled());
        assert_eq!(config.interval, UptimeReporterInterval::default());

        let duration_only = json!( {
            "interval": "2m",
        });

        let config: UptimeReporterConfig = serde_json::from_value(duration_only).unwrap();
        assert_eq!(
            config.interval,
            UptimeReporterInterval::from(Duration::from_secs(120))
        );
        assert!(config.enabled());
    }
}
