use duration_str::deserialize_duration;
use opentelemetry_sdk::logs;
use opentelemetry_sdk::trace;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug, time::Duration};
use url::Url;
use wrapper_with_default::WrapperWithDefault;

use crate::http::config::ProxyConfig;

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
/// Logs suffix for the OpenTelemetry endpoint
const LOGS_SUFFIX: &str = "/v1/logs";

/// Represents the OpenTelemetry configuration
#[derive(Debug, Deserialize, PartialEq, Clone)]
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
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,
    /// Client proxy configuration. It is supposed to take global proxy configuration, that's why it is skipped in
    /// serde serialization and deserialization.
    #[serde(skip)]
    pub(crate) proxy: ProxyConfig,
    /// Custom attributes to be added to all traces/metrics.
    #[serde(default)]
    pub(crate) custom_attributes: HashMap<String, String>,
}

fn default_insecure_level() -> String {
    DEFAULT_FILTER.to_string()
}

impl OtelConfig {
    /// Returns a new configuration including proxy config
    pub fn with_proxy_config(self, proxy: ProxyConfig) -> Self {
        Self { proxy, ..self }
    }

    /// Returns a new configuration including custom_attributes
    pub fn with_custom_attributes(self, custom_attributes: HashMap<String, String>) -> Self {
        Self {
            custom_attributes,
            ..self
        }
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
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Sets up the interval to report metrics. They are reported periodically according to it.
    #[serde(default)]
    pub(crate) interval: MetricsExportInterval,
}

/// Defines the configuration settings to report traces to OpenTelemetry
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct TracesConfig {
    /// Indicates if traces are enabled or not
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Traces are reported in batches, this field defines the batch configuration.
    #[serde(default)]
    pub(crate) batch_config: BatchConfig,
}

/// Defines the configuration settings to report logs to OpenTelemetry
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct LogsConfig {
    /// Indicates if logs are enabled or not
    #[serde(default)]
    pub(crate) enabled: bool,
    /// Traces are reported in batches, this field defines the batch configuration.
    #[serde(default)]
    pub(crate) batch_config: BatchConfig,
}

/// Type to represent a client timeout. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Type to represent the metrics export interval. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_METRICS_EXPORT_INTERVAL)]
pub struct MetricsExportInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Holds the batch configuration to send traces/logs telemetry data through OpenTelemetry.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub(crate) struct BatchConfig {
    #[serde(deserialize_with = "deserialize_duration")]
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const EXAMPLE_WITH_DEFAULTS_OPENTELEMETRY_CONFIG: &str = r#"
insecure_level: "newrelic_agent_control=info,off"
endpoint: https://otlp.nr-data.net:4318
metrics:
  enabled: true
traces:
  enabled: true
logs:
  enabled: true
"#;

    const EXAMPLE_FULLY_POPULATED_OPENTELEMETRY_CONFIG: &str = r#"
insecure_level: "newrelic_agent_control=info,off"
endpoint: https://otlp.nr-data.net:4318
headers: {}
client_timeout: 10s
custom_attributes:
    cluster_name: "test"
    environment: production
metrics:
  enabled: true
  interval: 120s
traces:
  enabled: true
  batch_config:
    scheduled_delay: 30s
    max_size: 512
logs:
  enabled: true
  batch_config:
    scheduled_delay: 30s
    max_size: 512
"#;

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
                custom_attributes: Default::default(),
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
    fn test_yaml_parsing() {
        assert!(
            serde_yaml::from_str::<OtelConfig>(EXAMPLE_WITH_DEFAULTS_OPENTELEMETRY_CONFIG).is_ok()
        );
        assert!(
            serde_yaml::from_str::<OtelConfig>(EXAMPLE_FULLY_POPULATED_OPENTELEMETRY_CONFIG)
                .is_ok()
        );
    }
}
