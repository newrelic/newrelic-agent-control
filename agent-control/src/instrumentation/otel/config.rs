use duration_str::deserialize_duration;
use opentelemetry_sdk::trace;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug, time::Duration};
use url::Url;

use crate::http::config::ProxyConfig;

/// Default timeout for HTTP client.
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default interval for exporting metrics.
const DEFAULT_METRICS_EXPORT_INTERVAL: Duration = Duration::from_secs(60);
/// Default maximum batch size [trace::BatchSpanProcessor] for details.
const DEFAULT_BATCH_MAX_SIZE: usize = 512;
/// Default scheduled delay [trace::BatchSpanProcessor] for details.
const DEFAULT_BATCH_SCHEDULED_DELAY: Duration = Duration::from_secs(30);

/// Represents the OpenTelemetry configuration
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct OtelConfig {
    /// Metrics configuration
    #[serde(default)]
    pub(crate) metrics: MetricsConfig,
    /// Traces configuration
    #[serde(default)]
    pub(crate) traces: TracesConfig,
    /// OpenTelemetry HTTP endpoint to report instrumentation.
    pub(crate) endpoint: Url,
    /// Headers to include in every request to the OpenTelemetry endpoint
    #[serde(default)]
    pub(crate) headers: HashMap<String, String>,
    /// Client timeout
    pub(crate) client_timeout: ClientTimeout,
    /// Client proxy configuration
    #[serde(skip)]
    pub(crate) proxy: ProxyConfig,
}

impl OtelConfig {
    /// Returns a new configuration including proxy config
    pub fn with_proxy_config(self, proxy: ProxyConfig) -> Self {
        Self { proxy, ..self }
    }
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct MetricsConfig {
    pub(crate) enabled: bool,
    pub(crate) interval: MetricsExportInterval,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct TracesConfig {
    pub(crate) enabled: bool,
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
