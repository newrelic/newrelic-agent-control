use crate::agent_type::runtime_config::health_config::{OnHostHealthCheck, OnHostHealthConfig};
use crate::health::health_checker::{HealthChecker, HealthCheckerError};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::http::client::HttpClient;
use std::path::PathBuf;

use super::file::FileHealthChecker;
use super::http::HttpHealthChecker;

pub enum OnHostHealthChecker {
    Http(HttpHealthChecker),
    File(FileHealthChecker),
}

impl OnHostHealthChecker {
    pub fn try_new(
        http_client: HttpClient,
        health_config: OnHostHealthConfig,
        start_time: StartTime,
    ) -> Result<Self, HealthCheckerError> {
        match health_config.check {
            OnHostHealthCheck::HttpHealth(http_config) => Ok(OnHostHealthChecker::Http(
                HttpHealthChecker::new(http_client, http_config, start_time)?,
            )),
            OnHostHealthCheck::FileHealth(file_config) => Ok(OnHostHealthChecker::File(
                FileHealthChecker::new(PathBuf::from(file_config.path)),
            )),
        }
    }
}

impl HealthChecker for OnHostHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            OnHostHealthChecker::Http(http_checker) => http_checker.check_health(),
            OnHostHealthChecker::File(file_checker) => file_checker.check_health(),
        }
    }
}
