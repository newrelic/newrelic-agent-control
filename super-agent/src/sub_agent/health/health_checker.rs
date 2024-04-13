use std::time::Duration;

use thiserror::Error;

use crate::agent_type::health_config::{HealthCheck, HealthConfig};

use super::http::HttpHealthChecker;

#[derive(Debug)]
pub enum Health {
    Healthy(Healthy),
    Unhealthy(Unhealthy),
}

impl From<Healthy> for Health {
    fn from(healthy: Healthy) -> Self {
        Health::Healthy(healthy)
    }
}

impl From<Unhealthy> for Health {
    fn from(unhealthy: Unhealthy) -> Self {
        Health::Unhealthy(unhealthy)
    }
}

/// A HealthCheckerError also means the agent is unhealthy.
impl From<HealthCheckerError> for Health {
    fn from(err: HealthCheckerError) -> Self {
        Health::Unhealthy(err.into())
    }
}

impl From<HealthCheckerError> for Unhealthy {
    fn from(err: HealthCheckerError) -> Self {
        Unhealthy {
            last_error: err.0,
            status: Default::default(),
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Healthy {
    pub status: String,
}

impl Healthy {
    pub fn status(&self) -> &str {
        &self.status
    }
}

/// Represents the unhealthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Unhealthy {
    pub status: String,
    pub last_error: String,
}

impl Unhealthy {
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }
}

impl Health {
    pub fn status(&self) -> &str {
        match self {
            Health::Healthy(healthy) => healthy.status(),
            Health::Unhealthy(unhealthy) => unhealthy.status(),
        }
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, Health::Healthy { .. })
    }

    pub fn last_error(&self) -> Option<&str> {
        if let Health::Unhealthy(unhealthy) = self {
            Some(unhealthy.last_error())
        } else {
            None
        }
    }
}

/// A type that implements a health checking mechanism.
pub trait HealthChecker {
    /// Check the health of the agent.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<Health, HealthCheckerError>;

    fn interval(&self) -> Duration;
}

pub(crate) enum HealthCheckerType {
    Http(HttpHealthChecker),
}

/// Health check errors.
#[derive(Debug, Error)]
#[error("Health check error: {0}")]
pub struct HealthCheckerError(String);

impl HealthCheckerError {
    pub fn new(err: String) -> Self {
        Self(err)
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
        }
    }
}

impl HealthChecker for HealthCheckerType {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.check_health(),
        }
    }

    fn interval(&self) -> Duration {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.interval(),
        }
    }
}
