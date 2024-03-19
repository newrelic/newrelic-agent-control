use super::health_checker::{HealthCheckError, HealthChecker};
use crate::agent_type::health_config::HealthConfig;
use std::time::Duration;

pub(crate) struct ExecHealthChecker {
    path: String,
    args: Vec<String>,
    interval: Duration,
    timeout: Duration,
}

impl ExecHealthChecker {
    pub fn new(path: String, args: Vec<String>, interval: Duration, timeout: Duration) -> Self {
        Self {
            path,
            args,
            interval,
            timeout,
        }
    }
}

impl HealthChecker for ExecHealthChecker {
    fn check_health(&self) -> Result<(), HealthCheckError> {
        Ok(())
    }

    fn interval(&self) -> Duration {
        self.interval
    }
}

impl From<HealthConfig> for ExecHealthChecker {
    fn from(config: HealthConfig) -> Self {
        let path = config.path;
        let args = config.args.unwrap_or_default();
        let interval = Duration::from_secs(config.interval.unwrap_or(30));
        let timeout = Duration::from_secs(config.timeout.unwrap_or(5));
        Self {
            path,
            args,
            interval,
            timeout,
        }
    }
}
