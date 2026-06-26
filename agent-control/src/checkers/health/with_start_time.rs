//! A health value paired with the agent's start time, convertible to OpAMP component health.
use super::health_checker::{Health, Healthy, Unhealthy};
use crate::utils::time::{sys_time_from_unix_timestamp, unix_timestamp_from_sys_time};
use std::time::SystemTime;

/// The start time of the agent whose health is reported.
pub type StartTime = SystemTime;

/// A [`Health`] value together with the agent's start time.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthWithStartTime {
    start_time: StartTime,
    health: Health,
}

impl From<HealthWithStartTime> for opamp_client::opamp::proto::ComponentHealth {
    fn from(h: HealthWithStartTime) -> Self {
        opamp_client::opamp::proto::ComponentHealth {
            start_time_unix_nano: unix_timestamp_from_sys_time(h.start_time),
            status_time_unix_nano: unix_timestamp_from_sys_time(h.status_time()),
            healthy: h.is_healthy(),
            status: h.status(),
            last_error: h.last_error().unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl From<opamp_client::opamp::proto::ComponentHealth> for HealthWithStartTime {
    fn from(component_health: opamp_client::opamp::proto::ComponentHealth) -> Self {
        let start_time = sys_time_from_unix_timestamp(component_health.start_time_unix_nano);
        let status_time = sys_time_from_unix_timestamp(component_health.status_time_unix_nano);

        if component_health.healthy {
            HealthWithStartTime::from_healthy(
                Healthy::new()
                    .with_status(component_health.status)
                    .with_status_time(status_time),
                start_time,
            )
        } else {
            HealthWithStartTime::from_unhealthy(
                Unhealthy::new(component_health.last_error).with_status(component_health.status),
                start_time,
            )
        }
    }
}

impl From<HealthWithStartTime> for Health {
    fn from(health_with_start_time: HealthWithStartTime) -> Self {
        health_with_start_time.health
    }
}

impl HealthWithStartTime {
    /// Builds a value from a health and a start time.
    pub fn new(health: Health, start_time: StartTime) -> Self {
        Self { health, start_time }
    }

    /// Builds a healthy value with the given start time.
    pub fn from_healthy(healthy: Healthy, start_time: StartTime) -> Self {
        HealthWithStartTime::new(healthy.into(), start_time)
    }

    /// Builds an unhealthy value with the given start time.
    pub fn from_unhealthy(unhealthy: Unhealthy, start_time: StartTime) -> Self {
        HealthWithStartTime::new(unhealthy.into(), start_time)
    }

    /// Returns `true` if the health is healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self.health, Health::Healthy { .. })
    }

    /// Returns the last error if unhealthy, `None` otherwise.
    pub fn last_error(&self) -> Option<String> {
        if let Health::Unhealthy(unhealthy) = &self.health {
            Some(unhealthy.last_error().to_string())
        } else {
            None
        }
    }

    /// Returns the agent-specific status message.
    pub fn status(&self) -> String {
        match &self.health {
            Health::Healthy(healthy) => healthy.status(),
            Health::Unhealthy(unhealthy) => unhealthy.status(),
        }
        .to_string()
    }

    /// Returns the agent's start time.
    pub fn start_time(&self) -> StartTime {
        self.start_time
    }

    /// Returns the time at which the status was determined.
    pub fn status_time(&self) -> StartTime {
        match &self.health {
            Health::Healthy(healthy) => healthy.status_time(),
            Health::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    /// Returns a reference to the wrapped [`Health`].
    pub fn as_health(&self) -> &Health {
        &self.health
    }
}
