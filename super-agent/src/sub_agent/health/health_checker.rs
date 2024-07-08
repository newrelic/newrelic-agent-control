use super::with_start_time::StartTime;
use crate::agent_type::health_config::HealthCheckInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
#[cfg(feature = "k8s")]
use crate::k8s;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::super_agent::config::AgentID;
use std::thread;
use std::time::{SystemTime, SystemTimeError};
use tracing::{debug, error};

pub type StatusTime = SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    Healthy(Healthy),
    Unhealthy(Unhealthy),
}

#[derive(thiserror::Error, Debug)]
pub enum HealthCheckerError {
    #[error("{0}")]
    Generic(String),
    #[error("system time error `{0}`")]
    SystemTime(#[from] SystemTimeError),
    #[cfg(feature = "k8s")]
    #[error("{kind}/{name} misses field `{field}`")]
    MissingK8sObjectField {
        kind: String,
        name: String,
        field: String,
    },
    #[cfg(feature = "k8s")]
    #[error("{kind}/{name} is invalid: {err}")]
    InvalidK8sObject {
        kind: String,
        name: String,
        err: String,
    },
    #[cfg(feature = "k8s")]
    #[error("k8s error: {0}")]
    K8sError(#[from] k8s::Error),
}

impl Health {
    pub fn is_healthy(&self) -> bool {
        matches!(self, Health::Healthy { .. })
    }

    pub fn last_error(&self) -> Option<&str> {
        if let Health::Unhealthy(unhealthy) = self {
            Some(unhealthy.last_error())
        } else {
            None
        }
    }

    pub fn status(&self) -> &str {
        match self {
            Health::Healthy(healthy) => healthy.status(),
            Health::Unhealthy(unhealthy) => unhealthy.status(),
        }
    }

    pub fn status_time(&self) -> StatusTime {
        match self {
            Health::Healthy(healthy) => healthy.status_time(),
            Health::Unhealthy(unhealthy) => unhealthy.status_time(),
        }
    }
}

impl From<Healthy> for Health {
    fn from(healthy: Healthy) -> Self {
        Health::Healthy(healthy)
    }
}

impl From<Unhealthy> for Health {
    fn from(unhealthy: Unhealthy) -> Self {
        Health::Unhealthy(unhealthy)
    }
}

/// A HealthCheckerError also means the agent is unhealthy.
impl From<HealthCheckerError> for Health {
    fn from(err: HealthCheckerError) -> Self {
        Health::Unhealthy(err.into())
    }
}

impl From<HealthCheckerError> for Unhealthy {
    fn from(err: HealthCheckerError) -> Self {
        Unhealthy::new("Health check error".to_string(), err.to_string())
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Clone)]
pub struct Healthy {
    pub(super) status_time: StatusTime,
    pub(super) status: String,
}

impl PartialEq for Healthy {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect any two status_time to be equal
        self.status == other.status
    }
}

impl Healthy {
    pub fn new(status: String) -> Self {
        Self {
            status,
            status_time: StatusTime::now(),
        }
    }
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_time(&self) -> StatusTime {
        self.status_time
    }
}

/// Represents the unhealthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Clone)]
pub struct Unhealthy {
    pub(super) status_time: StatusTime,
    pub(super) status: String,
    pub(super) last_error: String,
}

impl PartialEq for Unhealthy {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect any two status_time to be equal
        self.status == other.status && self.last_error == other.last_error
    }
}

impl Unhealthy {
    pub fn new(status: String, last_error: String) -> Self {
        Self {
            status,
            last_error,
            status_time: StatusTime::now(),
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    pub fn status_time(&self) -> StatusTime {
        self.status_time
    }
}

/// A type that implements a health checking mechanism.
pub trait HealthChecker {
    /// Check the health of the agent.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<Health, HealthCheckerError>;
}

pub(crate) fn spawn_health_checker<H>(
    agent_id: AgentID,
    health_checker: H,
    cancel_signal: EventConsumer<CancellationMessage>,
    health_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: HealthCheckInterval,
    start_time: StartTime,
) where
    H: HealthChecker + Send + 'static,
{
    thread::spawn(move || loop {
        if cancel_signal.is_cancelled(interval.into()) {
            break;
        }
        debug!(%agent_id, "starting to check health with the configured checker");
        let health = match health_checker.check_health() {
            Ok(health) => health,
            Err(err) => {
                debug!(%agent_id, last_error = %err, "the configured health check failed");
                Unhealthy::from(err).into()
            }
        };

        publish_health_event(
            &health_publisher,
            HealthWithStartTime::new(health, start_time).into(),
        );
    });
}

pub(crate) fn publish_health_event(
    internal_event_publisher: &EventPublisher<SubAgentInternalEvent>,
    event: SubAgentInternalEvent,
) {
    let event_type_str = format!("{:?}", event);
    _ = internal_event_publisher.publish(event).inspect_err(|e| {
        error!(
            err = e.to_string(),
            event_type = event_type_str,
            "could not publish sub agent event"
        )
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    impl Default for Healthy {
        fn default() -> Self {
            Self {
                status_time: StatusTime::UNIX_EPOCH,
                status: String::default(),
            }
        }
    }

    impl Default for Unhealthy {
        fn default() -> Self {
            Self {
                status_time: StatusTime::UNIX_EPOCH,
                status: String::default(),
                last_error: String::default(),
            }
        }
    }

    impl Unhealthy {
        pub fn with_last_error(self, last_error: String) -> Self {
            Self { last_error, ..self }
        }
    }

    mock! {
        pub HealthCheckMock{}
        impl HealthChecker for HealthCheckMock{
            fn check_health(&self) -> Result<Health, HealthCheckerError>;
        }
    }

    impl MockHealthCheckMock {
        pub fn new_healthy() -> MockHealthCheckMock {
            let mut healthy = MockHealthCheckMock::new();
            healthy
                .expect_check_health()
                .returning(|| Ok(Healthy::default().into()));
            healthy
        }

        pub fn new_unhealthy() -> MockHealthCheckMock {
            let mut unhealthy = MockHealthCheckMock::new();
            unhealthy
                .expect_check_health()
                .returning(|| Ok(Unhealthy::new(String::default(), String::default()).into()));
            unhealthy
        }

        pub fn new_with_error() -> MockHealthCheckMock {
            let mut unhealthy = MockHealthCheckMock::new();
            unhealthy
                .expect_check_health()
                .returning(|| Err(HealthCheckerError::Generic("test".to_string())));
            unhealthy
        }
    }
}
