use std::time::SystemTime;

use tracing::{debug, error, info_span};

use crate::{
    agent_control::agent_id::AgentID,
    event::{
        SubAgentInternalEvent,
        cancellation::CancellationMessage,
        channel::{EventConsumer, EventPublisher},
    },
    health::{
        health_checker::{HealthCheckInterval, HealthChecker, Unhealthy},
        with_start_time::{HealthWithStartTime, StartTime},
    },
    sub_agent::{identity::ID_ATTRIBUTE_NAME, supervisor::starter::SupervisorStarterError},
    utils::thread_context::{NotStartedThreadContext, StartedThreadContext},
};

const HEALTH_CHECKER_THREAD_NAME: &str = "health_checker";

pub(super) fn spawn_health_checker<H>(
    agent_id: AgentID,
    health_checker: H,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: HealthCheckInterval,
    sub_agent_start_time: StartTime,
) -> StartedThreadContext
where
    H: HealthChecker + Send + 'static,
{
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        let span = info_span!(
            "health_check",
            { ID_ATTRIBUTE_NAME } = %agent_id
        );
        let _guard = span.enter();

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

pub(super) fn publish_health_event(
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
pub(super) fn log_and_report_unhealthy(
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
mod tests {
    use mockall::Sequence;
    use std::time::{Duration, SystemTime};

    use crate::{
        agent_control::agent_id::AgentID,
        event::{SubAgentInternalEvent, channel::pub_sub},
        health::{
            health_checker::{HealthCheckerError, Healthy, Unhealthy, tests::MockHealthCheck},
            with_start_time::HealthWithStartTime,
        },
        sub_agent::health_checker::spawn_health_checker,
    };

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
            AgentID::default(),
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
            AgentID::default(),
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
            AgentID::default(),
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
