use std::time::SystemTime;

#[derive(Debug, PartialEq)]
pub enum HealthWithTimes {
    Healthy(HealthyWithTimes),
    Unhealthy(UnhealthyWithTimes),
}

impl From<HealthyWithTimes> for HealthWithTimes {
    fn from(health: HealthyWithTimes) -> Self {
        HealthWithTimes::Healthy(health)
    }
}

impl From<UnhealthyWithTimes> for HealthWithTimes {
    fn from(health: UnhealthyWithTimes) -> Self {
        HealthWithTimes::Unhealthy(health)
    }
}

impl HealthWithTimes {
    pub fn unhealthy_with_last_error(
        last_error: String,
        status: String,
        start_time: SystemTime,
    ) -> Self {
        Self::Unhealthy(UnhealthyWithTimes {
            last_error,
            status,
            start_time,
            status_time: SystemTime::now(),
        })
    }

    pub fn healthy(status: String, start_time: SystemTime) -> Self {
        Self::Healthy(HealthyWithTimes {
            status,
            start_time,
            status_time: SystemTime::now(),
        })
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthWithTimes::Healthy { .. })
    }

    pub fn last_error(&self) -> Option<&str> {
        if let HealthWithTimes::Unhealthy(unhealthy) = self {
            Some(unhealthy.last_error())
        } else {
            None
        }
    }

    pub fn status(&self) -> &str {
        match self {
            HealthWithTimes::Healthy(healthy) => healthy.status(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.status(),
        }
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        match self {
            HealthWithTimes::Healthy(healthy) => {
                HealthWithTimes::Healthy(healthy.with_status_time(start_time))
            }
            HealthWithTimes::Unhealthy(unhealthy) => {
                HealthWithTimes::Unhealthy(unhealthy.with_status_time(start_time))
            }
        }
    }

    pub fn start_time(&self) -> SystemTime {
        match self {
            HealthWithTimes::Healthy(healthy) => healthy.status_time(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    pub fn with_status_time(self, status_time: SystemTime) -> Self {
        match self {
            HealthWithTimes::Healthy(healthy) => {
                HealthWithTimes::Healthy(healthy.with_status_time(status_time))
            }
            HealthWithTimes::Unhealthy(unhealthy) => {
                HealthWithTimes::Unhealthy(unhealthy.with_status_time(status_time))
            }
        }
    }

    pub fn status_time(&self) -> SystemTime {
        match self {
            HealthWithTimes::Healthy(healthy) => healthy.status_time(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }

    pub fn is_same_without_times(&self, other: &Self) -> bool {
        match (self, other) {
            (HealthWithTimes::Healthy(healthy), HealthWithTimes::Healthy(other_healthy)) => {
                healthy.is_same_without_times(other_healthy)
            }
            (
                HealthWithTimes::Unhealthy(unhealthy),
                HealthWithTimes::Unhealthy(other_unhealthy),
            ) => unhealthy.is_same_without_times(other_unhealthy),
            _ => false,
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Clone)]
pub struct HealthyWithTimes {
    pub start_time: SystemTime,
    pub status_time: SystemTime,
    pub status: String,
}

impl PartialEq for HealthyWithTimes {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect the `status_time_unix_nano` to be the same across different instances.
        self.status == other.status && self.start_time == other.start_time
    }
}

impl HealthyWithTimes {
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
pub struct UnhealthyWithTimes {
    pub start_time: SystemTime,
    pub status_time: SystemTime,
    pub status: String,
    pub last_error: String,
}

impl PartialEq for UnhealthyWithTimes {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect the `status_time_unix_nano` to be the same across different instances.
        self.last_error == other.last_error
            && self.status == other.status
            && self.start_time == other.start_time
    }
}

impl UnhealthyWithTimes {
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
