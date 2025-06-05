use super::with_start_time::HealthWithStartTime;

/// This trait represents any event publisher that can publish health information.
pub trait HealthEventPublisher {
    fn publish_health_event(&self, health: HealthWithStartTime);
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
