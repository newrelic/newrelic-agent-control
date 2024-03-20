use std::time::Duration;

use thiserror::Error;

use crate::agent_type::health_config::{HealthCheck, HealthConfig};

use super::{exec::ExecHealthChecker, http::HttpHealthChecker};

/// A type that implements a health checking mechanism.
pub trait HealthChecker {
    type Error: std::error::Error;
    /// Check the health of the agent. `Ok(())` means the agent is healthy. Otherwise,
    /// we will have an `Err(e)` where `e` is the error with agent-specific semantics
    /// with which we will build the OpAMP's `ComponentHealth.status` contents.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<(), Self::Error>;

    fn interval(&self) -> Duration;
}

pub(crate) enum HealthCheckerType {
    Http(HttpHealthChecker),
    Exec(ExecHealthChecker),
}

#[derive(Debug, Error)]
pub enum HealthCheckerError {
    #[error(transparent)] // We forward the errors as is
    HttpError(std::fmt::Error),
    #[error(transparent)] // We forward the errors as is
    ExecError(std::fmt::Error),
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
    type Error = HealthCheckerError;
    fn check_health(&self) -> Result<(), Self::Error> {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker
                .check_health()
                .map_err(HealthCheckerError::HttpError),
            HealthCheckerType::Exec(exec_checker) => exec_checker
                .check_health()
                .map_err(HealthCheckerError::ExecError),
        }
    }

    fn interval(&self) -> Duration {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.interval(),
            HealthCheckerType::Exec(exec_checker) => exec_checker.interval(),
        }
    }
}
