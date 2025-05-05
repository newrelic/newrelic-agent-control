use super::with_start_time::StartTime;
use crate::agent_type::runtime_config::HealthCheckInterval;
use crate::event::SubAgentInternalEvent;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};

use crate::k8s;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::supervisor::starter::SupervisorStarterError;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use std::time::{SystemTime, SystemTimeError};
use tracing::{debug, error};

const HEALTH_CHECKER_THREAD_NAME: &str = "health checker";

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

    #[error("{kind}/{name} misses field `{field}`")]
    MissingK8sObjectField {
        kind: String,
        name: String,
        field: String,
    },

    #[error("{kind}/{name} is invalid: {err}")]
    InvalidK8sObject {
        kind: String,
        name: String,
        err: String,
    },

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
    health_checker: H,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: HealthCheckInterval,
    sub_agent_start_time: StartTime,
) -> StartedThreadContext
where
    H: HealthChecker + Send + 'static,
{
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        debug!("starting to check health with the configured checker");

        let health = health_checker.check_health().unwrap_or_else(|err| {
            debug!( last_error = %err, "the configured health check failed");
            HealthWithStartTime::from_unhealthy(Unhealthy::from(err), sub_agent_start_time)
        });

        publish_health_event(
            &sub_agent_internal_publisher,
            SubAgentInternalEvent::AgentHealthInfo(health),
        );

        // Check the cancellation signal
        if stop_consumer.is_cancelled(interval.into()) {
            break;
        }
    };

    NotStartedThreadContext::new(HEALTH_CHECKER_THREAD_NAME, callback).start()
}

pub(crate) fn publish_health_event(
    sub_agent_internal_publisher: &EventPublisher<SubAgentInternalEvent>,
    event: SubAgentInternalEvent,
) {
    let event_type_str = format!("{:?}", event);
    _ = sub_agent_internal_publisher
        .publish(event)
        .inspect_err(|e| {
            error!(
                err = e.to_string(),
                event_type = event_type_str,
                "could not publish sub agent event"
            )
        });
}

/// Logs the provided error and publishes the corresponding unhealthy event.
pub fn log_and_report_unhealthy(
    sub_agent_internal_publisher: &EventPublisher<SubAgentInternalEvent>,
    err: &SupervisorStarterError,
    msg: &str,
    start_time: SystemTime,
) {
    let last_error = format!("{msg}: {err}");

    let event = SubAgentInternalEvent::AgentHealthInfo(HealthWithStartTime::new(
        Unhealthy::new(String::default(), last_error).into(),
        start_time,
    ));

    error!(%err, msg);
    publish_health_event(sub_agent_internal_publisher, event);
}

#[cfg(test)]
pub mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::event::channel::pub_sub;

    use super::*;
    use mockall::{Sequence, mock};

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
        pub HealthCheck{}
        impl HealthChecker for HealthCheck{
            fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError>;
        }
    }

    impl MockHealthCheck {
        pub fn new_healthy() -> MockHealthCheck {
            let mut healthy = MockHealthCheck::new();
            healthy.expect_check_health().returning(|| {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::default(),
                    UNIX_EPOCH,
                ))
            });
            healthy
        }

        pub fn new_unhealthy() -> MockHealthCheck {
            let mut unhealthy = MockHealthCheck::new();
            unhealthy.expect_check_health().returning(|| {
                Ok(HealthWithStartTime::from_unhealthy(
                    Unhealthy::new(String::default(), String::default()),
                    UNIX_EPOCH,
                ))
            });
            unhealthy
        }

        pub fn new_with_error() -> MockHealthCheck {
            let mut unhealthy = MockHealthCheck::new();
            unhealthy
                .expect_check_health()
                .returning(|| Err(HealthCheckerError::Generic("test".to_string())));
            unhealthy
        }
    }

    #[test]
    fn test_spawn_health_checker() {
        let (health_publisher, health_consumer) = pub_sub();

        let start_time = SystemTime::now();

        let mut health_checker = MockHealthCheck::new();
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
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let started_thread_context = spawn_health_checker(
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            SubAgentInternalEvent::from(HealthWithStartTime::new(
                Healthy::new("status: 0".to_string()).into(),
                start_time
            )),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            SubAgentInternalEvent::from(HealthWithStartTime::new(
                Unhealthy::new(
                    "Health check error".to_string(),
                    "mocked health check error!".to_string(),
                )
                .into(),
                start_time,
            )),
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }

    #[test]
    fn test_repeating_healthy() {
        let (health_publisher, health_consumer) = pub_sub();

        let start_time = SystemTime::now();

        let mut health_checker = MockHealthCheck::new();
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
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new("status: 1".to_string()),
                    start_time,
                ))
            });

        let started_thread_context = spawn_health_checker(
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            SubAgentInternalEvent::from(HealthWithStartTime::new(
                Healthy::new("status: 0".to_string()).into(),
                start_time
            )),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            SubAgentInternalEvent::from(HealthWithStartTime::new(
                Healthy::new("status: 1".to_string()).into(),
                start_time
            )),
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }

    #[test]
    fn test_repeating_unhealthy() {
        let (health_publisher, health_consumer) = pub_sub();

        let mut health_checker = MockHealthCheck::new();
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
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let start_time = SystemTime::now();

        let started_thread_context = spawn_health_checker(
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        let expected_health_event = SubAgentInternalEvent::from(HealthWithStartTime::new(
            Unhealthy::new(
                "Health check error".to_string(),
                "mocked health check error!".to_string(),
            )
            .into(),
            start_time,
        ));
        assert_eq!(
            expected_health_event.clone(),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            expected_health_event,
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }
}
