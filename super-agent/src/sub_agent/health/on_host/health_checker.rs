use std::path::PathBuf;

use crate::agent_type::health_config::{OnHostHealthCheck, OnHostHealthConfig};
use crate::sub_agent::health::health_checker::{HealthChecker, HealthCheckerError};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};

use super::file::FileHealthChecker;
use super::http::HttpHealthChecker;

pub enum HealthCheckerType {
    Http(HttpHealthChecker),
    File(FileHealthChecker),
}

impl HealthCheckerType {
    pub fn try_new(
        health_config: OnHostHealthConfig,
        start_time: StartTime,
    ) -> Result<Self, HealthCheckerError> {
        let timeout = health_config.timeout;

        match health_config.check {
            OnHostHealthCheck::HttpHealth(http_config) => Ok(HealthCheckerType::Http(
                HttpHealthChecker::new(timeout, http_config, start_time)?,
            )),
            OnHostHealthCheck::FileHealth(file_config) => Ok(HealthCheckerType::File(
                FileHealthChecker::new(PathBuf::from(file_config.path)),
            )),
        }
    }
}

impl HealthChecker for HealthCheckerType {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.check_health(),
            HealthCheckerType::File(file_checker) => file_checker.check_health(),
        }
    }
}
