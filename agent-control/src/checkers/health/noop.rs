use std::time::SystemTime;

use super::{
    health_checker::{HealthChecker, HealthCheckerError, Healthy},
    with_start_time::HealthWithStartTime,
};

/// HealthCheckerBuilder that always return None.
/// No-op implementation for [HealthChecker]. It always returns healthy.
pub struct NoOpHealthChecker {
    start_time: SystemTime,
}

impl NoOpHealthChecker {
    pub fn new(start_time: SystemTime) -> Self {
        Self { start_time }
    }
}

impl HealthChecker for NoOpHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        Ok(HealthWithStartTime::from_healthy(
            Healthy::new(),
            self.start_time,
        ))
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    impl Default for NoOpHealthChecker {
        fn default() -> Self {
            Self {
                start_time: SystemTime::UNIX_EPOCH,
            }
        }
    }
}
