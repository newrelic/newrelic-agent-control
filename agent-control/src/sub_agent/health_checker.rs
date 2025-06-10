use tracing::error;

use crate::{
    event::{SubAgentInternalEvent, channel::EventPublisher},
    health::{events::HealthEventPublisher, with_start_time::HealthWithStartTime},
};

impl HealthEventPublisher for EventPublisher<SubAgentInternalEvent> {
    fn publish_health_event(&self, health: HealthWithStartTime) {
        let event = SubAgentInternalEvent::AgentHealthInfo(health);
        let event_type = format!("{event:?}");
        _ = self.publish(event).inspect_err(|err| {
            error!(%event_type, "Error publishing sub agent event: {err}");
        })
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::SystemTime;

    use assert_matches::assert_matches;
    use tracing_test::traced_test;

    use crate::{
        event::{SubAgentInternalEvent, channel::pub_sub},
        health::{
            events::HealthEventPublisher, health_checker::Healthy,
            with_start_time::HealthWithStartTime,
        },
    };

    #[test]
    fn test_publish_health_event() {
        let (publisher, consumer) = pub_sub::<SubAgentInternalEvent>();
        let health = HealthWithStartTime::new(Healthy::default().into(), SystemTime::now());
        publisher.publish_health_event(health.clone());
        let event = consumer.as_ref().recv().unwrap();
        assert_matches!(event, SubAgentInternalEvent::AgentHealthInfo(h) => {
            assert_eq!(health, h);
        });
    }

    #[traced_test]
    #[test]
    fn test_publish_health_event_error() {
        let (publisher, consumer) = pub_sub::<SubAgentInternalEvent>();
        // Drop the consumer to close the channel
        drop(consumer);

        let health = HealthWithStartTime::new(Healthy::default().into(), SystemTime::now());
        publisher.publish_health_event(health.clone());

        logs_contain("Error publishing sub agent event:");
    }
}
