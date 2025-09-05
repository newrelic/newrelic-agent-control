use crate::agent_control::agent_id::AgentID;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::EventConsumer;
use crate::health::events::HealthEventPublisher;
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::k8s;
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::thread::sleep;
use std::time::{Duration, SystemTime, SystemTimeError};
use tracing::{debug, error, info_span};
use wrapper_with_default::WrapperWithDefault;

pub const HEALTH_CHECKER_THREAD_NAME: &str = "health_checker";

const DEFAULT_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_INITIAL_DELAY: Duration = Duration::ZERO;

pub type StatusTime = SystemTime;

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_HEALTH_CHECK_INTERVAL)]
pub struct HealthCheckInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_INITIAL_DELAY)]
pub struct InitialDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);

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
        Unhealthy::new(err.to_string()).with_status("Health check error".to_string())
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
    /// Returns a new instance with the current status_time
    #[allow(clippy::new_without_default)] // The corresponding default implementation would have `now` as value of `status_time`
    pub fn new() -> Self {
        Self {
            status: Default::default(),
            status_time: StatusTime::now(),
        }
    }

    pub fn with_status(self, status: String) -> Self {
        Self { status, ..self }
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
    /// Returns a new instance with the current status_time and the provided `last_error`
    pub fn new(last_error: String) -> Self {
        Self {
            status: Default::default(),
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

    pub fn with_status(self, status: String) -> Self {
        Self { status, ..self }
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

    /// Checks health and perform retries if the result is unhealthy or there was an error executing health check.
    /// The retries are performed as specified by provided limit and retry_interval.
    fn check_health_with_retry(
        &self,
        limit: u64,
        retry_interval: Duration,
    ) -> Result<HealthWithStartTime, HealthCheckerError> {
        let mut last_health = Err(HealthCheckerError::Generic("initial value".into()));
        for attempt in 1..=limit {
            debug!("Checking health with retries {attempt}/{limit}");
            last_health = self.check_health();
            match last_health.as_ref() {
                Ok(health) => {
                    if health.is_healthy() {
                        debug!("Health check result was healthy");
                        return last_health;
                    }
                    if let Some(err) = health.last_error() {
                        debug!("Health check result was unhealthy: {err}");
                    }
                }
                Err(err) => {
                    debug!("Failure to check health: {err}");
                }
            }
            sleep(retry_interval);
        }
        last_health
    }
}

/// Spawns a thread that periodically checks health of an agent and publishes the results.
///
/// The thread runs health checks at the specified interval using the provided `health_checker`.
/// Results are published through the given `event_publisher`.
///
/// # Arguments
/// * `agent_id` - The ID of the agent whose health is checked
/// * `health_checker` - The health checker implementation
/// * `event_publisher` - Publisher for health events
/// * `interval` - Duration between health checks
/// * `sub_agent_start_time` - The start time of the sub-agent
pub(crate) fn spawn_health_checker<H, E>(
    agent_id: AgentID,
    health_checker: H,
    event_publisher: E,
    interval: HealthCheckInterval,
    initial_delay: InitialDelay,
    start_time: StartTime,
) -> StartedThreadContext
where
    H: HealthChecker + Send + 'static,
    E: HealthEventPublisher,
{
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
        debug!("Starting to check health with the configured checker");

        sleep(initial_delay.into());

        loop {
            let span = info_span!(
                "health_check",
                { ID_ATTRIBUTE_NAME } = %agent_id
            );
            let _guard = span.enter();

            debug!("Checking health");
            let health = health_checker.check_health().unwrap_or_else(|err| {
                debug!(last_error = %err, "The configured health check failed");
                HealthWithStartTime::from_unhealthy(Unhealthy::from(err), start_time)
            });

            event_publisher.publish_health_event(health);

            // Check the cancellation signal
            if stop_consumer.is_cancelled(interval.into()) {
                break;
            }
        }
    };
    NotStartedThreadContext::new(HEALTH_CHECKER_THREAD_NAME, callback).start()
}

#[cfg(test)]
pub mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::event::channel::pub_sub;

    use super::*;
    use assert_matches::assert_matches;
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
                    Healthy::new(),
                    UNIX_EPOCH,
                ))
            });
            healthy
        }

        pub fn new_unhealthy() -> MockHealthCheck {
            let mut unhealthy = MockHealthCheck::new();
            unhealthy.expect_check_health().returning(|| {
                Ok(HealthWithStartTime::from_unhealthy(
                    Unhealthy::new(String::default()),
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
    fn test_health_check_with_retry_success_on_first_attempt() {
        let health_checker = MockHealthCheck::new_healthy();

        let result = health_checker.check_health_with_retry(3, Duration::from_millis(10));

        assert_matches!(result, Ok(health) => {
            assert!(health.is_healthy());
        });
    }

    #[test]
    fn test_health_check_with_retry_success_after_retries() {
        let mut health_checker = MockHealthCheck::new();
        let mut seq = Sequence::new();

        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                Err(HealthCheckerError::Generic(
                    "error on first attempt".to_string(),
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                Ok(HealthWithStartTime::from_unhealthy(
                    Unhealthy::new("Unhealthy on second attempt".to_string())
                        .with_status("Unhealthy".to_string()),
                    UNIX_EPOCH,
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new(),
                    UNIX_EPOCH,
                ))
            });

        let result = health_checker.check_health_with_retry(3, Duration::from_millis(10));

        assert_matches!(result, Ok(health) => {
            assert!(health.is_healthy());
        });
    }

    #[test]
    fn test_health_check_with_retry_failure_after_all_attempts() {
        let mut health_checker = MockHealthCheck::new();
        health_checker
            .expect_check_health()
            .times(3)
            .returning(|| Err(HealthCheckerError::Generic("persistent error".to_string())));

        let result = health_checker.check_health_with_retry(3, Duration::from_millis(10));

        assert_matches!(result, Err(HealthCheckerError::Generic(s)) => {
            assert_eq!(s, "persistent error".to_string());
        });
    }

    #[test]
    fn test_health_check_with_retry_unhealthy_result() {
        let mut health_checker = MockHealthCheck::new();
        health_checker.expect_check_health().times(3).returning(|| {
            Ok(HealthWithStartTime::from_unhealthy(
                Unhealthy::new("persistent unhealthy".to_string())
                    .with_status("Unhealthy".to_string()),
                UNIX_EPOCH,
            ))
        });

        let result = health_checker.check_health_with_retry(3, Duration::from_millis(10));

        assert_matches!(result, Ok(health) => {
            assert!(!health.is_healthy());
            assert_eq!(health.last_error().unwrap(), "persistent unhealthy".to_string());
        });
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
                    Healthy::new().with_status("status: 0".to_string()),
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
            AgentID::default(),
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            InitialDelay::default(),
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            HealthWithStartTime::new(
                Healthy::new().with_status("status: 0".to_string()).into(),
                start_time
            ),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            HealthWithStartTime::new(
                Unhealthy::new("mocked health check error!".to_string(),)
                    .with_status("Health check error".to_string())
                    .into(),
                start_time,
            ),
            health_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }
    #[test]
    fn test_spawn_health_checker_initial_delay() {
        let (health_publisher, health_consumer) = pub_sub();

        let start_time = SystemTime::now();

        let mut health_checker = MockHealthCheck::new();
        health_checker
            .expect_check_health()
            .once()
            .returning(move || {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new().with_status("status: 0".to_string()),
                    start_time,
                ))
            });

        let initial_delay = Duration::from_millis(500);

        let started_thread_context = spawn_health_checker(
            AgentID::default(),
            health_checker,
            health_publisher,
            Duration::MAX.into(), // retries are not tested here
            initial_delay.into(),
            start_time,
        );

        let health: HealthWithStartTime = health_consumer.as_ref().recv().unwrap();

        // Check that initial delay has been honored
        assert!(
            SystemTime::now()
                .duration_since(health.start_time())
                .unwrap()
                >= initial_delay
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
                    Healthy::new().with_status("status: 0".to_string()),
                    start_time,
                ))
            });
        health_checker
            .expect_check_health()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(HealthWithStartTime::from_healthy(
                    Healthy::new().with_status("status: 1".to_string()),
                    start_time,
                ))
            });

        let started_thread_context = spawn_health_checker(
            AgentID::default(),
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            InitialDelay::default(),
            start_time,
        );

        // Check that we received the two expected health events
        assert_eq!(
            HealthWithStartTime::new(
                Healthy::new().with_status("status: 0".to_string()).into(),
                start_time
            ),
            health_consumer.as_ref().recv().unwrap()
        );
        assert_eq!(
            HealthWithStartTime::new(
                Healthy::new().with_status("status: 1".to_string()).into(),
                start_time
            ),
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
            AgentID::default(),
            health_checker,
            health_publisher,
            Duration::from_millis(10).into(), // Give room to publish and consume the events
            InitialDelay::default(),
            start_time,
        );

        // Check that we received the two expected health events
        let expected_health_event = HealthWithStartTime::new(
            Unhealthy::new("mocked health check error!".to_string())
                .with_status("Health check error".to_string())
                .into(),
            start_time,
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
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(health_consumer.as_ref().recv().is_err());
    }
}
