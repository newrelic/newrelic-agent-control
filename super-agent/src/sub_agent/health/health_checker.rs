use crate::agent_type::health_config::HealthCheckInterval;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
use crate::super_agent::config::AgentID;
use std::thread;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use tracing::{debug, error};

#[cfg(feature = "k8s")]
use crate::k8s;

#[derive(Debug, PartialEq)]
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
        Unhealthy {
            last_error: format!("Health check error: {}", err),
            ..Default::default()
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone)]
pub struct Healthy {
    pub start_time_unix_nano: u64,
    pub status_time_unix_nano: u64,
    pub status: String,
}

impl PartialEq for Healthy {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect any two status_time_unix_nano to be equal
        self.start_time_unix_nano == other.start_time_unix_nano && self.status == other.status
    }
}

impl Healthy {
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
#[derive(Debug, Default, Clone)]
pub struct Unhealthy {
    pub start_time_unix_nano: u64,
    pub status_time_unix_nano: u64,
    pub status: String,
    pub last_error: String,
}

impl PartialEq for Unhealthy {
    fn eq(&self, other: &Self) -> bool {
        // We cannot expect any two status_time_unix_nano to be equal
        self.start_time_unix_nano == other.start_time_unix_nano
            && self.status == other.status
            && self.last_error == other.last_error
    }
}

impl Unhealthy {
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

impl Health {
    pub fn unhealthy_with_last_error(last_error: String) -> Self {
        Self::Unhealthy(Unhealthy {
            last_error,
            ..Default::default()
        })
    }

    pub fn healthy() -> Self {
        Self::Healthy(Healthy {
            ..Default::default()
        })
    }

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

    pub fn with_start_time_unix_nano(self, start_time_unix_nano: u64) -> Self {
        match self {
            Health::Healthy(healthy) => {
                Health::Healthy(healthy.with_status_time_unix_nano(start_time_unix_nano))
            }
            Health::Unhealthy(unhealthy) => {
                Health::Unhealthy(unhealthy.with_status_time_unix_nano(start_time_unix_nano))
            }
        }
    }

    pub fn start_time_unix_nano(&self) -> u64 {
        match self {
            Health::Healthy(healthy) => healthy.status_time_unix_nano(),
            Health::Unhealthy(unhealthy) => unhealthy.status_time_unix_nano(),
        }
    }

    pub fn with_status_time_unix_nano(self, status_time_unix_nano: u64) -> Self {
        match self {
            Health::Healthy(healthy) => {
                Health::Healthy(healthy.with_status_time_unix_nano(status_time_unix_nano))
            }
            Health::Unhealthy(unhealthy) => {
                Health::Unhealthy(unhealthy.with_status_time_unix_nano(status_time_unix_nano))
            }
        }
    }

    pub fn status_time_unix_nano(&self) -> u64 {
        match self {
            Health::Healthy(healthy) => healthy.status_time_unix_nano(),
            Health::Unhealthy(unhealthy) => unhealthy.status_time_unix_nano(),
        }
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
    cancel_signal: EventConsumer<()>,
    health_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: HealthCheckInterval,
) where
    H: HealthChecker + Send + 'static,
{
    let start_time_unix_nano = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .inspect_err(|e| error!("error getting agent start time: {}. Setting to 0", e))
        .unwrap_or_default()
        .as_nanos() as u64;

    thread::spawn(move || loop {
        thread::sleep(interval.into());

        // Check cancellation signal.
        // As we don't need any data to be sent, the `publish` call of the sender only sends `()`
        // and we don't check for data here, We use a non-blocking call and break only if we
        // received the message successfully.
        if cancel_signal.as_ref().try_recv().is_ok() {
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

        let status_time_unix_nano = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .inspect_err(|e| error!("error getting agent status time: {}. Setting to 0.", e))
            .unwrap_or_default()
            .as_nanos() as u64;

        publish_health_event(
            &health_publisher,
            health
                .with_start_time_unix_nano(start_time_unix_nano)
                .with_status_time_unix_nano(status_time_unix_nano)
                .into(),
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
pub mod test {
    use super::*;
    use mockall::mock;

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
                .returning(|| Ok(Unhealthy::default().into()));
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
