use http::HttpHealthChecker;

use crate::agent_type::health_config::{OnHostHealthCheck, OnHostHealthConfig};

use super::health_checker::{Health, HealthChecker, HealthCheckerError};

pub mod http;

pub enum HealthCheckerType {
    Http(HttpHealthChecker),
}

impl TryFrom<OnHostHealthConfig> for HealthCheckerType {
    type Error = HealthCheckerError;

    fn try_from(health_config: OnHostHealthConfig) -> Result<Self, Self::Error> {
        let timeout = health_config.timeout;

        match health_config.check {
            OnHostHealthCheck::HttpHealth(http_config) => Ok(HealthCheckerType::Http(
                HttpHealthChecker::new(timeout, http_config)?,
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
}
