use std::time::SystemTime;

use tracing::error;

use crate::health::health_checker::Unhealthy;

use super::with_start_time::HealthWithStartTime;

/// This trait represents any event publisher that can publish health information.
pub trait HealthEventPublisher {
    fn publish_health_event(&self, health: HealthWithStartTime);

    /// Logs the provided error and published the corresponding unhealthy event
    fn log_error_and_publish_unhealthy(
        &self,
        err: impl std::error::Error,
        msg: &str,
        start_time: SystemTime,
    ) {
        let last_error = format!("{msg}: {err}");
        error!("{}", &last_error);
        let health = HealthWithStartTime::new(
            Unhealthy::new(String::default(), last_error).into(),
            start_time,
        );
        self.publish_health_event(health);
    }
}

#[cfg(test)]
pub mod tests {
    use crate::event::channel::EventPublisher;

    use super::*;

    /// Dummy implementation to make testing easier
    impl HealthEventPublisher for EventPublisher<HealthWithStartTime> {
        fn publish_health_event(&self, health: HealthWithStartTime) {
            self.publish(health).unwrap();
        }
    }
}
