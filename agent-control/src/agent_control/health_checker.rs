use serde::Deserialize;
use tracing::error;

use crate::{
    event::{AgentControlInternalEvent, channel::EventPublisher},
    health::{
        events::HealthEventPublisher, health_checker::HealthCheckInterval,
        with_start_time::HealthWithStartTime,
    },
};

/// Holds the Agent Control configuration fields for setting up the health-check.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct AgentControlHealthCheckerConfig {
    pub interval: HealthCheckInterval,
}

impl HealthEventPublisher for EventPublisher<AgentControlInternalEvent> {
    fn publish_health_event(&self, health: HealthWithStartTime) {
        let event = AgentControlInternalEvent::HealthUpdated(health);
        let event_type = format!("{event:?}");
        let _ = self.publish(event).map_err(|err| {
            error!(%event_type, "Error publishing Agent Control internal event {err}");
        });
    }
}
