use super::with_start_time::StartTime;
use crate::agent_control::config::{AgentID, AgentTypeFQN};
use crate::agent_type::health_config::HealthCheckInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentEvent;
#[cfg(feature = "k8s")]
use crate::k8s;
use crate::sub_agent::event_handler::on_health::on_health;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::thread_context::{NotStartedThreadContext, StartedThreadContext};
use opamp_client::StartedClient;
use std::sync::Arc;
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

pub(crate) fn spawn_health_checker<H, C>(
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    health_checker: H,
    maybe_opamp_client: Arc<Option<C>>,
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    interval: HealthCheckInterval,
    sub_agent_start_time: StartTime,
) -> StartedThreadContext
where
    H: HealthChecker + Send + 'static,
    C: StartedClient + Send + Sync + 'static,
{
    let agent_id_clone = agent_id.clone();
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        debug!(agent_id = %agent_id_clone, "starting to check health with the configured checker");

        let health = health_checker.check_health().unwrap_or_else(|err| {
            debug!(agent_id = %agent_id_clone, last_error = %err, "the configured health check failed");
            HealthWithStartTime::from_unhealthy(Unhealthy::from(err), sub_agent_start_time)
        });

        let _ = on_health(
                                    health.clone(),
                            maybe_opamp_client.clone(),
                                    sub_agent_publisher.clone(),
                                    agent_id_clone.clone(),
                                    agent_type.clone(),
                                )
                                .inspect_err(|e| error!(error = %e, select_arm = "sub_agent_internal_consumer", "processing health message"));

        // Check the cancellation signal
        if stop_consumer.is_cancelled(interval.into()) {
            break;
        }
    };

    NotStartedThreadContext::new(agent_id, "health checker", callback).start()
}

#[cfg(test)]
pub mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{
        event::channel::pub_sub, opamp::client_builder::tests::MockStartedOpAMPClientMock,
    };

    use crate::agent_control::config::AgentTypeFQN;

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
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap();
        let started_thread_context = spawn_health_checker(
            agent_id.clone(),
            agent_type.clone(),
            health_checker,
            Arc::new(None::<MockStartedOpAMPClientMock>),
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            SubAgentEvent::SubAgentHealthInfo(
                agent_id.clone(),
                agent_type.clone(),
                HealthWithStartTime::new(Healthy::new("status: 0".to_string()).into(), start_time)
            ),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            SubAgentEvent::SubAgentHealthInfo(
                agent_id,
                agent_type,
                HealthWithStartTime::new(
                    Unhealthy::new(
                        "Health check error".to_string(),
                        "mocked health check error!".to_string(),
                    )
                    .into(),
                    start_time,
                )
            ),
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }

    #[test]
    fn test_repeating_healthy() {
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
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new("status: 1".to_string()),
                    start_time,
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap();
        let started_thread_context = spawn_health_checker(
            agent_id.clone(),
            agent_type.clone(),
            health_checker,
            Arc::new(None::<MockStartedOpAMPClientMock>),
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            SubAgentEvent::SubAgentHealthInfo(
                agent_id.clone(),
                agent_type.clone(),
                HealthWithStartTime::new(Healthy::new("status: 0".to_string()).into(), start_time)
            ),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            SubAgentEvent::SubAgentHealthInfo(
                agent_id,
                agent_type,
                HealthWithStartTime::new(Healthy::new("status: 1".to_string()).into(), start_time)
            ),
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }

    #[test]
    fn test_repeating_unhealthy() {
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
                Err(HealthCheckerError::Generic(
                    "mocked health check error!".to_string(),
                ))
            });

        let start_time = SystemTime::now();

        let agent_id = AgentID::new("test-agent").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap();
        let started_thread_context = spawn_health_checker(
            agent_id.clone(),
            agent_type.clone(),
            health_checker,
            Arc::new(None::<MockStartedOpAMPClientMock>),
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            start_time,
        );

        // Check that we received the two expected health events
        let expected_health_event = SubAgentEvent::SubAgentHealthInfo(
            agent_id.clone(),
            agent_type.clone(),
            HealthWithStartTime::new(
                Unhealthy::new(
                    "Health check error".to_string(),
                    "mocked health check error!".to_string(),
                )
                .into(),
                start_time,
            ),
        );
        assert_eq!(
            expected_health_event.clone(),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            expected_health_event,
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }
}
