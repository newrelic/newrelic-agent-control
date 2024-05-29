use crate::agent_type::health_config::HealthCheckInterval;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
use crate::super_agent::config::AgentID;
use std::thread;
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
            status: Default::default(),
        }
    }
}

/// Represents the healthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Healthy {
    pub status: String,
}

impl Healthy {
    pub fn status(&self) -> &str {
        &self.status
    }
}

/// Represents the unhealthy state of the agent and its associated data.
/// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
/// for more details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Unhealthy {
    pub status: String,
    pub last_error: String,
}

impl Unhealthy {
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }
}

impl Health {
    pub fn unhealthy_with_last_error(last_error: String) -> Health {
        Health::Unhealthy(Unhealthy {
            last_error,
            ..Default::default()
        })
    }

    pub fn status(&self) -> &str {
        match self {
            Health::Healthy(healthy) => healthy.status(),
            Health::Unhealthy(unhealthy) => unhealthy.status(),
        }
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
        match health_checker.check_health() {
            Ok(health) => publish_health_event(&health_publisher, health.into()),
            Err(err) => {
                debug!(%agent_id, last_error = %err, "the configured health check failed");
                publish_health_event(&health_publisher, Unhealthy::from(err).into())
            }
        }
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
