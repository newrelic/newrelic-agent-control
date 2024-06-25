use std::time::SystemTime;

use super::health_checker::{Health, Healthy, Unhealthy};

pub type StartTime = SystemTime;

#[derive(Debug, PartialEq)]
pub struct HealthWithStartTime {
    start_time: StartTime,
    health: Health,
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
