use std::time::Duration;

use crate::agent_type::health_config::{HealthCheck, HealthConfig};

use super::{exec::ExecHealthChecker, http::HttpHealthChecker};

/// A type that implements a health checking mechanism.
pub trait HealthChecker {
    /// Check the health of the agent. `Ok(())` means the agent is healthy. Otherwise,
    /// we will have an `Err(e)` where `e` is the error with agent-specific semantics
    /// with which we will build the OpAMP's `ComponentHealth.status` contents.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<(), HealthCheckerError>;

    fn interval(&self) -> Duration;
}

pub(crate) enum HealthCheckerType {
    Http(HttpHealthChecker),
    Exec(ExecHealthChecker),
}

/// Health check errors. Its structure mimics the OpAMP's spec for containing relevant information
#[derive(Debug)]
pub struct HealthCheckerError {
    /// Status contents using agent-specific semantics. This might be the response body of an HTTP
    /// checker or the stdout/stderr of an exec checker.
    status: String,

    /// Error information in human-readable format. We could use this to specify what kind of checker
    /// failed, e.g., "HTTP checker failed with error: {error}". While passing the raw error to the
    /// `status` field.
    last_error: String,
}

impl HealthCheckerError {
    pub fn new(status: String, last_error: String) -> Self {
        Self { status, last_error }
    }

    pub fn status(self) -> String {
        self.status
    }

    pub fn last_error(self) -> String {
        self.last_error
    }
}

impl TryFrom<HealthConfig> for HealthCheckerType {
    type Error = HealthCheckerError;

    fn try_from(health_config: HealthConfig) -> Result<Self, Self::Error> {
        let interval = health_config.interval;
        let timeout = health_config.timeout;

        match health_config.check {
            HealthCheck::HttpHealth(http_config) => Ok(HealthCheckerType::Http(
                HttpHealthChecker::new(interval, timeout, http_config)?,
            )),
            HealthCheck::ExecHealth(exec_config) => Ok(HealthCheckerType::Exec(
                ExecHealthChecker::new(interval, timeout, exec_config),
            )),
        }
    }
}

impl HealthChecker for HealthCheckerType {
    fn check_health(&self) -> Result<(), HealthCheckerError> {
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
