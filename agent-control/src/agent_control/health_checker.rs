use serde::Deserialize;

use crate::health::health_checker::HealthCheckInterval;

/// Holds the Agent Control configuration fields for setting up the health-check.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct AgentControlHealthCheckerConfig {
    interval: HealthCheckInterval,
}
