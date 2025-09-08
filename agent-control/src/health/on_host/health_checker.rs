use super::exec::ExecHealthChecker;
use super::file::FileHealthChecker;
use super::http::HttpHealthChecker;
use crate::agent_type::runtime_config::health_config::{OnHostHealthCheck, OnHostHealthConfig};
use crate::event::channel::EventConsumer;
use crate::health::health_checker::{HealthChecker, HealthCheckerError};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::http::client::HttpClient;
use std::path::PathBuf;

pub enum OnHostHealthChecker {
    Exec(ExecHealthChecker),
    Http(HttpHealthChecker),
    File(FileHealthChecker),
}

impl OnHostHealthChecker {
    pub fn try_new(
        exec_health_repository: EventConsumer<(String, HealthWithStartTime)>,
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
            _ => Ok(OnHostHealthChecker::Exec(ExecHealthChecker::new(
                exec_health_repository,
            ))),
        }
    }
}

impl HealthChecker for OnHostHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            OnHostHealthChecker::Exec(exec_checker) => exec_checker.check_health(),
            OnHostHealthChecker::Http(http_checker) => http_checker.check_health(),
            OnHostHealthChecker::File(file_checker) => file_checker.check_health(),
        }
    }
}
