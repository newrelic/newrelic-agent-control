use crate::agent_type::health_config::ExecHealth;

use super::health_checker::{HealthChecker, HealthCheckerError};
use std::time::Duration;

#[derive(Debug, Default)]
pub(crate) struct ExecHealthChecker {
    path: String,
    args: Vec<String>,
    healthy_exit_codes: Vec<i32>,
    interval: Duration,
    timeout: Duration,
}

impl ExecHealthChecker {
    pub fn new(interval: Duration, timeout: Duration, exec_config: ExecHealth) -> Self {
        let path = exec_config.path;
        let args = exec_config.args;
        let healthy_exit_codes = exec_config.healthy_exit_codes;
        Self {
            path,
            args,
            healthy_exit_codes,
            interval,
            timeout,
        }
    }
}

impl HealthChecker for ExecHealthChecker {
    fn check_health(&self) -> Result<(), HealthCheckerError> {
        Ok(())
    }

    fn interval(&self) -> Duration {
        self.interval
    }
}
