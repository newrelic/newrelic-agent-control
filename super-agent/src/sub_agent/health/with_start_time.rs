use super::health_checker::{Health, Healthy, Unhealthy};
use crate::utils::time::{sys_time_from_unix_timestamp, unix_timestamp_from_sys_time};
use std::time::SystemTime;

pub type StartTime = SystemTime;

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
                Healthy::new(component_health.status).with_status_time(status_time),
                start_time,
            )
        } else {
            HealthWithStartTime::from_unhealthy(
                Unhealthy::new(component_health.status, component_health.last_error),
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
    pub fn new(health: Health, start_time: StartTime) -> Self {
        Self { health, start_time }
    }

    pub fn from_healthy(healthy: Healthy, start_time: StartTime) -> Self {
        HealthWithStartTime::new(healthy.into(), start_time)
    }

    pub fn from_unhealthy(unhealthy: Unhealthy, start_time: StartTime) -> Self {
        HealthWithStartTime::new(unhealthy.into(), start_time)
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.health, Health::Healthy { .. })
    }

    pub fn last_error(&self) -> Option<String> {
        if let Health::Unhealthy(unhealthy) = &self.health {
            Some(unhealthy.last_error().to_string())
        } else {
            None
        }
    }

    pub fn status(&self) -> String {
        match &self.health {
            Health::Healthy(healthy) => healthy.status(),
            Health::Unhealthy(unhealthy) => unhealthy.status(),
        }
        .to_string()
    }

    pub fn start_time(&self) -> StartTime {
        self.start_time
    }

    pub fn status_time(&self) -> StartTime {
        match &self.health {
            Health::Healthy(healthy) => healthy.status_time(),
            Health::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    pub fn is_same_without_times(&self, other: &Self) -> bool {
        self.health == other.health
    }
}
