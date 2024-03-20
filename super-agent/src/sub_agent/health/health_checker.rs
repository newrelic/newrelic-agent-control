use std::time::Duration;

use crate::agent_type::health_config::{HealthCheck, HealthConfig};

use super::{exec::ExecHealthChecker, http::HttpHealthChecker};

pub(super) type HealthCheckError = String;

/// A type that implements a health checking mechanism.
pub trait HealthChecker: Send {
    /// Check the health of the agent. `Ok(())` means the agent is healthy. Otherwise,
    /// we will have an `Err(e)` where `e` is the error with agent-specific semantics
    /// with which we will build the OpAMP's `ComponentHealth.status` contents.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<(), HealthCheckError>;

    fn interval(&self) -> Duration;
}

pub(crate) enum HealthCheckerType {
    Http(HttpHealthChecker),
    Exec(ExecHealthChecker),
}

impl From<HealthConfig> for HealthCheckerType {
    fn from(health_config: HealthConfig) -> Self {
        let interval = health_config.interval;
        let timeout = health_config.timeout;
        match health_config.check {
            HealthCheck::HttpHealth(http_config) => {
                HealthCheckerType::Http(HttpHealthChecker::new(interval, timeout, http_config))
            }
            HealthCheck::ExecHealth(exec_config) => {
                HealthCheckerType::Exec(ExecHealthChecker::new(interval, timeout, exec_config))
            }
        }
    }
}

impl HealthChecker for HealthCheckerType {
    fn check_health(&self) -> Result<(), HealthCheckError> {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.check_health(),
            HealthCheckerType::Exec(exec_checker) => exec_checker.check_health(),
        }
    }

    fn interval(&self) -> Duration {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.interval(),
            HealthCheckerType::Exec(exec_checker) => exec_checker.interval(),
        }
    }
}
