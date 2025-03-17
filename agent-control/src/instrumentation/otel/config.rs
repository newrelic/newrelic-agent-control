use duration_str::deserialize_duration;
use opentelemetry_sdk::trace;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug, time::Duration};
use url::Url;

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
