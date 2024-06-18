use super::health_checker::{Health, Healthy, Unhealthy};

#[derive(Debug, PartialEq)]
pub enum HealthWithTimes {
    Healthy(HealthyWithTimes),
    Unhealthy(UnhealthyWithTimes),
}

impl From<Health> for HealthWithTimes {
    fn from(health: Health) -> Self {
        match health {
            Health::Healthy(healthy) => Self::Healthy(healthy.into()),
            Health::Unhealthy(unhealthy) => Self::Unhealthy(unhealthy.into()),
        }
    }
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
    pub fn unhealthy_with_last_error(last_error: String) -> Self {
        Self::Unhealthy(UnhealthyWithTimes {
            last_error,
            ..Default::default()
        })
    }

    pub fn healthy() -> Self {
        Self::Healthy(HealthyWithTimes {
            ..Default::default()
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

    pub fn with_start_time_unix_nano(self, start_time_unix_nano: u64) -> Self {
        match self {
            HealthWithTimes::Healthy(healthy) => {
                HealthWithTimes::Healthy(healthy.with_status_time_unix_nano(start_time_unix_nano))
            }
            HealthWithTimes::Unhealthy(unhealthy) => HealthWithTimes::Unhealthy(
                unhealthy.with_status_time_unix_nano(start_time_unix_nano),
            ),
        }
    }

    pub fn start_time_unix_nano(&self) -> u64 {
        match self {
            HealthWithTimes::Healthy(healthy) => healthy.status_time_unix_nano(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.status_time_unix_nano(),
        }
    }

    pub fn with_status_time_unix_nano(self, status_time_unix_nano: u64) -> Self {
        match self {
            HealthWithTimes::Healthy(healthy) => {
                HealthWithTimes::Healthy(healthy.with_status_time_unix_nano(status_time_unix_nano))
            }
            HealthWithTimes::Unhealthy(unhealthy) => HealthWithTimes::Unhealthy(
                unhealthy.with_status_time_unix_nano(status_time_unix_nano),
            ),
        }
    }

    pub fn status_time_unix_nano(&self) -> u64 {
        match self {
            HealthWithTimes::Healthy(healthy) => healthy.status_time_unix_nano(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.status_time_unix_nano(),
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HealthyWithTimes {
    pub start_time_unix_nano: u64,
    pub status_time_unix_nano: u64,
    pub status: String,
}

impl From<Healthy> for HealthyWithTimes {
    fn from(healthy: Healthy) -> Self {
        Self {
            status: healthy.status,
            ..Default::default()
        }
    }
}

impl HealthyWithTimes {
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn with_start_time_unix_nano(self, start_time_unix_nano: u64) -> Self {
        Self {
            start_time_unix_nano,
            ..self
        }
    }

    pub fn start_time_unix_nano(&self) -> u64 {
        self.start_time_unix_nano
    }

    pub fn with_status_time_unix_nano(self, status_time_unix_nano: u64) -> Self {
        Self {
            status_time_unix_nano,
            ..self
        }
    }

    pub fn status_time_unix_nano(&self) -> u64 {
        self.status_time_unix_nano
    }
}

/// Represents the unhealthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct UnhealthyWithTimes {
    pub start_time_unix_nano: u64,
    pub status_time_unix_nano: u64,
    pub status: String,
    pub last_error: String,
}

impl From<Unhealthy> for UnhealthyWithTimes {
    fn from(unhealthy: Unhealthy) -> Self {
        Self {
            last_error: unhealthy.last_error,
            ..Default::default()
        }
    }
}

impl UnhealthyWithTimes {
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    pub fn with_start_time_unix_nano(self, start_time_unix_nano: u64) -> Self {
        Self {
            start_time_unix_nano,
            ..self
        }
    }

    pub fn start_time_unix_nano(&self) -> u64 {
        self.start_time_unix_nano
    }

    pub fn with_status_time_unix_nano(self, status_time_unix_nano: u64) -> Self {
        Self {
            status_time_unix_nano,
            ..self
        }
    }

    pub fn status_time_unix_nano(&self) -> u64 {
        self.status_time_unix_nano
    }
}
