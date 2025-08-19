pub mod handler;
pub mod k8s;
pub mod onhost;

use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use std::fmt::Debug;
use tracing::{debug, error, info, info_span, warn};
const VERSION_CHECKER_THREAD_NAME: &str = "version_checker";

pub trait VersionChecker {
    /// Use it to report the agent version for the opamp client
    /// Uses a thread to check the version of an agent and report it
    /// with internal events. The reported AgentVersion should
    /// contain "version" and the field for opamp that is going to contain the version
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentVersion {
    pub version: String,
    pub opamp_field: String,
}

#[derive(thiserror::Error, Debug)]
pub enum VersionCheckError {
    #[error("Generic error: {0}")]
    Generic(String),
}

pub(crate) fn spawn_version_checker<V, T, F>(
    version_checker_id: String,
    version_checker: V,
    version_event_publisher: EventPublisher<T>,
    version_event_generator: F,
    interval: VersionCheckerInterval,
) -> StartedThreadContext
where
    V: VersionChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(AgentVersion) -> T + Send + Sync + 'static,
{
    let thread_name = format!("{version_checker_id}_{VERSION_CHECKER_THREAD_NAME}");
    // Stores if the version was retrieved in last iteration for logging purposes.
    let mut version_retrieved = false;
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        let span = info_span!(
            "version_check",
            { ID_ATTRIBUTE_NAME } = %version_checker_id
        );
        let _guard = span.enter();

        debug!("starting to check version with the configured checker");

        match version_checker.check_agent_version() {
            Ok(agent_data) => {
                if !version_retrieved {
                    info!("agent version successfully checked");
                    version_retrieved = true;
                }

                publish_version_event(
                    &version_event_publisher,
                    version_event_generator(agent_data),
                );
            }
            Err(error) => {
                warn!("failed to check agent version: {error}");
                version_retrieved = false;
            }
        }

        if stop_consumer.is_cancelled(interval.into()) {
            break;
        }
    };

    NotStartedThreadContext::new(thread_name, callback).start()
}

pub(crate) fn publish_version_event<T>(version_event_publisher: &EventPublisher<T>, event: T)
where
    T: Debug + Send + Sync + 'static,
{
    let event_type_str = format!("{event:?}");
    _ = version_event_publisher.publish(event).inspect_err(|e| {
        error!(
            err = e.to_string(),
            event_type = event_type_str,
            "could not publish version event"
        )
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::defaults::OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY;
    use crate::event::SubAgentInternalEvent::AgentVersionInfo;
    use crate::event::channel::pub_sub;
    use crate::{agent_control::agent_id::AgentID, event::SubAgentInternalEvent};
    use mockall::{Sequence, mock};
    use std::time::Duration;

    mock! {
        pub VersionChecker {}
        impl VersionChecker for VersionChecker {
            fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
        }
    }

    #[test]
    fn test_spawn_version_checker() {
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionChecker::new();
        let mut seq = Sequence::new();
        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Err(VersionCheckError::Generic(
                    "mocked version check error!".to_string(),
                ))
            });
        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                Ok(AgentVersion {
                    version: "1.0.0".to_string(),
                    opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                })
            });

        let started_thread_context = spawn_version_checker(
            AgentID::default().as_str().to_string(),
            version_checker,
            version_publisher,
            SubAgentInternalEvent::AgentVersionInfo,
            Duration::from_millis(10).into(),
        );

        // Check that we received the expected version event
        assert_eq!(
            AgentVersionInfo(AgentVersion {
                version: "1.0.0".to_string(),
                opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            }),
            version_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop_blocking().unwrap();

        // Check there are no more events
        assert!(version_consumer.as_ref().recv().is_err());
    }
}
