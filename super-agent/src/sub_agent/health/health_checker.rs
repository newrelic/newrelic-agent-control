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
        // We cannot expect any two status_time to be equal, so we don't compare them
        let Self {
            status_time: _,
            status,
        } = self;
        let Self {
            status_time: _,
            status: other_status,
        } = other;

        status == other_status
    }
}

impl Healthy {
    pub fn new(status: String) -> Self {
        Self {
            status,
            status_time: StatusTime::now(),
        }
    }
    pub fn with_status_time(self, status_time: StatusTime) -> Self {
        Self {
            status_time,
            ..self
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
        // We cannot expect any two status_time to be equal, so we don't compare them
        let Self {
            status_time: _,
            status,
            last_error,
        } = self;
        let Self {
            status_time: _,
            status: other_status,
            last_error: other_last_error,
        } = other;

        status == other_status && last_error == other_last_error
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

    pub fn with_status_time(self, status_time: StatusTime) -> Self {
        Self {
            status_time,
            ..self
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
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError>;
}

pub(crate) fn spawn_health_checker<H>(
    agent_id: AgentID,
    health_checker: H,
    cancel_signal: EventConsumer<CancellationMessage>,
    health_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: HealthCheckInterval,
    sub_agent_start_time: StartTime,
) where
    H: HealthChecker + Send + 'static,
{
    thread::spawn(move || loop {
        if cancel_signal.is_cancelled(interval.into()) {
            break;
        }
        debug!(%agent_id, "starting to check health with the configured checker");

        let health = health_checker.check_health().unwrap_or_else(|err| {
            debug!(%agent_id, last_error = %err, "the configured health check failed");
            HealthWithStartTime::from_unhealthy(Unhealthy::from(err), sub_agent_start_time)
        });

        publish_health_event(&health_publisher, health.into());
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
    use std::time::{Duration, UNIX_EPOCH};

    use crate::event::channel::pub_sub;

    use super::*;
    use mockall::{mock, Sequence};

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
            fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError>;
        }
    }

    impl MockHealthCheckMock {
        pub fn new_healthy() -> MockHealthCheckMock {
            let mut healthy = MockHealthCheckMock::new();
            healthy.expect_check_health().returning(|| {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::default(),
                    UNIX_EPOCH,
                ))
            });
            healthy
        }

        pub fn new_unhealthy() -> MockHealthCheckMock {
            let mut unhealthy = MockHealthCheckMock::new();
            unhealthy.expect_check_health().returning(|| {
                Ok(HealthWithStartTime::from_unhealthy(
                    Unhealthy::new(String::default(), String::default()),
                    UNIX_EPOCH,
                ))
            });
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

    #[test]
    fn test_spawn_health_checker() {
        let (cancel_publisher, cancel_signal) = pub_sub();
        let (health_publisher, health_consumer) = pub_sub();

        let start_time = SystemTime::now();

        let mut health_checker = MockHealthCheckMock::new();
        let mut seq = Sequence::new();
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new("status: 0".to_string()),
                    start_time,
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                // Ensure the health checker will quit after the second loop
                cancel_publisher.publish(()).unwrap();
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        spawn_health_checker(
            agent_id,
            health_checker,
            cancel_signal,
            health_publisher,
            Duration::default().into(),
            start_time,
        );

        // Check that the health checker was called at least once
        let expected_health_events: Vec<SubAgentInternalEvent> = {
            vec![
                HealthWithStartTime::new(Healthy::new("status: 0".to_string()).into(), start_time)
                    .into(),
                HealthWithStartTime::new(
                    Unhealthy::new(
                        "Health check error".to_string(),
                        "mocked health check error!".to_string(),
                    )
                    .into(),
                    start_time,
                )
                .into(),
            ]
        };
        let actual_health_events = health_consumer.as_ref().iter().collect::<Vec<_>>();

        assert_eq!(expected_health_events, actual_health_events);
    }

    #[test]
    fn test_repeating_healthy() {
        let (cancel_publisher, cancel_signal) = pub_sub();
        let (health_publisher, health_consumer) = pub_sub();

        let start_time = SystemTime::now();

        let mut health_checker = MockHealthCheckMock::new();
        let mut seq = Sequence::new();
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new("status: 0".to_string()),
                    start_time,
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                // Ensure the health checker will quit after the second loop
                cancel_publisher.publish(()).unwrap();
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new("status: 1".to_string()),
                    start_time,
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();

        spawn_health_checker(
            agent_id,
            health_checker,
            cancel_signal,
            health_publisher,
            Duration::default().into(),
            start_time,
        );

        // Check that the health checker was called at least once
        let expected_health_events: Vec<SubAgentInternalEvent> = vec![
            HealthWithStartTime::new(Healthy::new("status: 0".to_string()).into(), start_time)
                .into(),
            HealthWithStartTime::new(Healthy::new("status: 1".to_string()).into(), start_time)
                .into(),
        ];
        let actual_health_events = health_consumer.as_ref().iter().collect::<Vec<_>>();

        assert_eq!(expected_health_events, actual_health_events);
    }

    #[test]
    fn test_repeating_unhealthy() {
        let (cancel_publisher, cancel_signal) = pub_sub();
        let (health_publisher, health_consumer) = pub_sub();

        let mut health_checker = MockHealthCheckMock::new();
        let mut seq = Sequence::new();
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                // Ensure the health checker will quit after the second loop
                cancel_publisher.publish(()).unwrap();
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let start_time = SystemTime::now();

        let agent_id = AgentID::new("test-agent").unwrap();
        spawn_health_checker(
            agent_id,
            health_checker,
            cancel_signal,
            health_publisher,
            Duration::default().into(),
            start_time,
        );

        // Check that the health checker was called at least once
        let expected_health_events: Vec<SubAgentInternalEvent> = {
            vec![
                HealthWithStartTime::new(
                    Unhealthy::new(
                        "Health check error".to_string(),
                        "mocked health check error!".to_string(),
                    )
                    .into(),
                    start_time,
                )
                .into(),
                HealthWithStartTime::new(
                    Unhealthy::new(
                        "Health check error".to_string(),
                        "mocked health check error!".to_string(),
                    )
                    .into(),
                    start_time,
                )
                .into(),
            ]
        };
        let actual_health_events = health_consumer.as_ref().iter().collect::<Vec<_>>();

        assert_eq!(expected_health_events, actual_health_events);
    }
}
