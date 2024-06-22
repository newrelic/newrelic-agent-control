use std::time::SystemTime;

use super::health_checker::{Health, Healthy, Unhealthy};

#[derive(Debug, PartialEq)]
pub enum HealthWithStartTime {
    Healthy(HealthyWithStartTime),
    Unhealthy(UnhealthyWithStartTime),
}

impl From<HealthyWithStartTime> for HealthWithStartTime {
    fn from(health: HealthyWithStartTime) -> Self {
        HealthWithStartTime::Healthy(health)
    }
}

impl From<UnhealthyWithStartTime> for HealthWithStartTime {
    fn from(health: UnhealthyWithStartTime) -> Self {
        HealthWithStartTime::Unhealthy(health)
    }
}

impl HealthWithStartTime {
    pub fn from_health(health: Health, start_time: SystemTime) -> Self {
        match health {
            Health::Healthy(healthy) => HealthWithStartTime::from_healthy(healthy, start_time),
            Health::Unhealthy(unhealthy) => {
                HealthWithStartTime::from_unhealthy(unhealthy, start_time)
            }
        }
    }

    pub fn from_healthy(healthy: Healthy, start_time: SystemTime) -> Self {
        HealthWithStartTime::Healthy(HealthyWithStartTime::new(healthy, start_time))
    }

    pub fn from_unhealthy(unhealthy: Unhealthy, start_time: SystemTime) -> Self {
        HealthWithStartTime::Unhealthy(UnhealthyWithStartTime::new(unhealthy, start_time))
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthWithStartTime::Healthy { .. })
    }

    pub fn last_error(&self) -> Option<&str> {
        if let HealthWithStartTime::Unhealthy(unhealthy) = self {
            Some(unhealthy.last_error())
        } else {
            None
        }
    }

    pub fn status(&self) -> &str {
        match self {
            HealthWithStartTime::Healthy(healthy) => healthy.status(),
            HealthWithStartTime::Unhealthy(unhealthy) => unhealthy.status(),
        }
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        match self {
            HealthWithStartTime::Healthy(healthy) => {
                HealthWithStartTime::Healthy(healthy.with_status_time(start_time))
            }
            HealthWithStartTime::Unhealthy(unhealthy) => {
                HealthWithStartTime::Unhealthy(unhealthy.with_status_time(start_time))
            }
        }
    }

    pub fn start_time(&self) -> SystemTime {
        match self {
            HealthWithStartTime::Healthy(healthy) => healthy.status_time(),
            HealthWithStartTime::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    pub fn with_status_time(self, status_time: SystemTime) -> Self {
        match self {
            HealthWithStartTime::Healthy(healthy) => {
                HealthWithStartTime::Healthy(healthy.with_status_time(status_time))
            }
            HealthWithStartTime::Unhealthy(unhealthy) => {
                HealthWithStartTime::Unhealthy(unhealthy.with_status_time(status_time))
            }
        }
    }

    pub fn status_time(&self) -> SystemTime {
        match self {
            HealthWithStartTime::Healthy(healthy) => healthy.status_time(),
            HealthWithStartTime::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    pub fn is_same_without_times(&self, other: &Self) -> bool {
        match (self, other) {
            (
                HealthWithStartTime::Healthy(healthy),
                HealthWithStartTime::Healthy(other_healthy),
            ) => healthy.is_same_without_times(other_healthy),
            (
                HealthWithStartTime::Unhealthy(unhealthy),
                HealthWithStartTime::Unhealthy(other_unhealthy),
            ) => unhealthy.is_same_without_times(other_unhealthy),
            _ => false,
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Clone)]
pub struct HealthyWithStartTime {
    start_time: SystemTime,
    status_time: SystemTime,
    status: String,
}

impl PartialEq for HealthyWithStartTime {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect the `status_time_unix_nano` to be the same across different instances.
        self.status == other.status && self.start_time == other.start_time
    }
}

impl HealthyWithStartTime {
    pub fn new(
        Healthy {
            status,
            status_time,
        }: Healthy,
        start_time: SystemTime,
    ) -> Self {
        Self {
            status,
            start_time,
            status_time,
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        Self { start_time, ..self }
    }

    pub fn start_time(&self) -> SystemTime {
        self.start_time
    }

    pub fn with_status_time(self, status_time: SystemTime) -> Self {
        Self {
            status_time,
            ..self
        }
    }

    pub fn status_time(&self) -> SystemTime {
        self.status_time
    }

    pub fn is_same_without_times(&self, other: &Self) -> bool {
        self.status == other.status
    }
}

/// Represents the unhealthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Clone)]
pub struct UnhealthyWithStartTime {
    start_time: SystemTime,
    status_time: SystemTime,
    status: String,
    last_error: String,
}

impl PartialEq for UnhealthyWithStartTime {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect the `status_time_unix_nano` to be the same across different instances.
        self.last_error == other.last_error
            && self.status == other.status
            && self.start_time == other.start_time
    }
}

impl UnhealthyWithStartTime {
    pub fn new(
        Unhealthy {
            status_time,
            status,
            last_error,
        }: Unhealthy,
        start_time: SystemTime,
    ) -> Self {
        Self {
            last_error,
            status,
            start_time,
            status_time,
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        Self { start_time, ..self }
    }

    pub fn start_time(&self) -> SystemTime {
        self.start_time
    }

    pub fn with_status_time(self, status_time: SystemTime) -> Self {
        Self {
            status_time,
            ..self
        }
    }

    pub fn status_time(&self) -> SystemTime {
        self.status_time
    }

    pub fn is_same_without_times(&self, other: &Self) -> bool {
        self.last_error == other.last_error && self.status == other.status
    }
}
