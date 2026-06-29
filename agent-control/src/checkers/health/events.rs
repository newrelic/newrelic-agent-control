//! Publishing of health events produced by health checkers.
use super::with_start_time::HealthWithStartTime;

/// This trait represents any event publisher that can publish health information.
pub trait HealthEventPublisher: Send + 'static {
    /// Publishes a health event.
    fn publish_health_event(&self, health: HealthWithStartTime);
}

#[cfg(test)]
#[allow(missing_docs)]
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
