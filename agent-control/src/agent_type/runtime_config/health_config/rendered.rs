//! Health-check configuration for on-host agents after templating.
use std::collections::HashMap;

use crate::agent_type::runtime_config::health_config::{
    FileHealth, HealthCheckTimeout, HttpHost, HttpPath, HttpPort,
};
use crate::checkers::health::health_checker::{HealthCheckInterval, InitialDelay};

/// Rendered on-host health-check configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct OnHostHealthConfig {
    /// The duration to wait between health checks.
    pub(crate) interval: HealthCheckInterval,
    /// The initial delay before the first health check is performed.
    pub(crate) initial_delay: InitialDelay,
    /// The maximum duration a health check may run before considered failed.
    pub(crate) timeout: HealthCheckTimeout,
    /// The list of health checks to run. Empty means health reporting is disabled.
    pub(crate) checks: Vec<OnHostHealthCheckDefinition>,
}

/// A single rendered on-host health check.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OnHostHealthCheckDefinition {
    /// Process probe.
    Process,
    /// HTTP endpoint probe.
    Http(HttpHealth),
    /// File-based probe.
    File(FileHealth),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HttpHealth {
    pub(crate) host: HttpHost,
    /// The HTTP path to check for the health check.
    pub(crate) path: HttpPath,
    /// The port to be checked during the health check.
    pub(crate) port: HttpPort,
    /// Optional HTTP headers to be included during the health check.
    pub(crate) headers: HashMap<String, String>,
    // allowed healthy HTTP status codes
    pub(crate) healthy_status_codes: Vec<u16>,
}
