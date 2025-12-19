use std::collections::HashMap;

use crate::agent_type::runtime_config::health_config::{
    FileHealth, HealthCheckTimeout, HttpHost, HttpPath, HttpPort,
};
use crate::checkers::health::health_checker::{HealthCheckInterval, InitialDelay};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OnHostHealthConfig {
    /// The duration to wait between health checks.
    pub(crate) interval: HealthCheckInterval,
    /// The initial delay before the first health check is performed.
    pub(crate) initial_delay: InitialDelay,
    /// The maximum duration a health check may run before considered failed.
    pub(crate) timeout: HealthCheckTimeout,
    /// Details on the type of health check. Defined by the `HealthCheck` enumeration.
    pub(crate) check: Option<OnHostHealthCheck>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OnHostHealthCheck {
    HttpHealth(HttpHealth),
    FileHealth(FileHealth),
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
