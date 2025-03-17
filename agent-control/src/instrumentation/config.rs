use crate::http::config::ProxyConfig;

use super::otel::config::OtelConfig;
use serde::{Deserialize, Serialize};

/// Represents the the configuration for instrumenting the application, excluding logs which
/// are directly configured through the [logs] module.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct InstrumentationConfig {
    pub(crate) opentelemetry: Option<OtelConfig>,
}

impl InstrumentationConfig {
    pub fn with_proxy_config(self, proxy: ProxyConfig) -> Self {
        Self {
            opentelemetry: self
                .opentelemetry
                .map(|otel_config| otel_config.with_proxy_config(proxy)),
        }
    }
}
