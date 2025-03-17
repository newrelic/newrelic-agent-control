use super::otel::config::OtelConfig;
use serde::{Deserialize, Serialize};

/// Represents the the configuration for instrumenting the application, excluding logs which
/// are directly configured through the [logs] module.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct InstrumentationConfig {
    pub(crate) opentelemetry: Option<OtelConfig>,
}
