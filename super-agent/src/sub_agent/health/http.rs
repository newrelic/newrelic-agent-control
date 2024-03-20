use super::health_checker::{HealthCheckError, HealthChecker};
use crate::agent_type::health_config::HttpHealth;
use std::time::Duration;

#[derive(Debug, Default)]
pub(crate) struct HttpHealthChecker {
    host: String,
    path: String,
    port: u16,
    interval: Duration,
    timeout: Duration,
}

impl HttpHealthChecker {
    pub fn new(interval: Duration, timeout: Duration, http_config: HttpHealth) -> Self {
        let host = http_config.host.get().into();
        let path = http_config.path.get().into();
        let port = http_config.port.get().into();
        let headers = http_config.headers;
        let healthy_status_codes = http_config.healthy_status_codes;
        Self {
            host,
            path,
            port,
            interval,
            timeout,
        }
    }
}

impl HealthChecker for HttpHealthChecker {
    fn check_health(&self) -> Result<(), HealthCheckError> {
        Ok(())
    }

    fn interval(&self) -> Duration {
        self.interval
    }
}
