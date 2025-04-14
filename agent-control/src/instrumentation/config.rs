//! The config module defines the configuration for the Agent Control instrumentation.
//!
//! It includes two different structures that can be set up separately.
//!
//! ```yaml
//! logs: # 'regular' logging configuration
//! instrumentation: # application self-instrumentaiton
//! ```

use crate::http::config::ProxyConfig;
use serde::Deserialize;

pub mod logs;
pub mod otel;

/// Represents the the configuration for instrumenting the application.
/// It does not include _regular logs_ configuration, which are directly configured through the [logs]
/// module, but it can also report logs with a different set of filtering and exporters.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct InstrumentationConfig {
    pub(crate) opentelemetry: Option<otel::OtelConfig>,
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
