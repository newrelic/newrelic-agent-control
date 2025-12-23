use std::time::Duration;

use serde::Deserialize;
use tracing::error;

use crate::checkers::health::{
    events::HealthEventPublisher,
    health_checker::{HealthCheckInterval, InitialDelay},
    with_start_time::HealthWithStartTime,
};
use crate::event::{AgentControlInternalEvent, channel::EventPublisher};

pub mod k8s;

/// Holds the Agent Control configuration fields for setting up the health-check.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentControlHealthCheckerConfig {
    /// The duration to wait between health checks.
    pub interval: HealthCheckInterval,
    /// The initial delay before the first health check is performed.
    pub initial_delay: InitialDelay,
}

impl Default for AgentControlHealthCheckerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30).into(),
            initial_delay: Duration::from_secs(30).into(),
        }
    }
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
