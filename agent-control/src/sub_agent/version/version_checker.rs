use crate::agent_control::agent_id::AgentID;
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use tracing::{debug, error, info, warn};

const HEALTH_CHECKER_THREAD_NAME: &str = "version checker";

pub trait VersionChecker {
    /// Use it to report the agent version for the opamp client
    /// Uses a thread to check the version of and agent and report it
    /// with internal events. The reported AgentVersion should
    /// contain "version" and the field for opamp that is going to contain the version
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentVersion {
    version: String,
    opamp_field: String,
}

impl AgentVersion {
    pub fn new(version: String, opamp_field: String) -> Self {
        Self {
            version,
            opamp_field,
        }
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn opamp_field(&self) -> &str {
        &self.opamp_field
    }
}

#[derive(thiserror::Error, Debug)]
pub enum VersionCheckError {
    #[error("Generic error: {0}")]
    Generic(String),
}

pub(crate) fn spawn_version_checker<V>(
    agent_id: AgentID,
    version_checker: V,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: VersionCheckerInterval,
) -> StartedThreadContext
where
    V: VersionChecker + Send + Sync + 'static,
{
    // Stores if the version was retrieved in last iteration for logging purposes.
    let mut version_retrieved = false;
    let agent_id_clone = agent_id.clone();
    let callback = move |stop_consumer: EventConsumer<CancellationMessage>| loop {
        debug!(agent_id = %agent_id_clone, "starting to check version with the configured checker");

        match version_checker.check_agent_version() {
            Ok(agent_data) => {
                if !version_retrieved {
                    info!(agent_id = %agent_id_clone, "agent version successfully checked");
                    version_retrieved = true;
                }

                publish_version_event(
                    &sub_agent_internal_publisher,
                    SubAgentInternalEvent::AgentVersionInfo(agent_data),
                );
            }
            Err(error) => {
                warn!(agent_id = %agent_id_clone, %error, "failed to check agent version");
                version_retrieved = false;
            }
        }

        if stop_consumer.is_cancelled(interval.into()) {
            break;
        }
    };

    info!(%agent_id, "{} started", HEALTH_CHECKER_THREAD_NAME);
    NotStartedThreadContext::new(HEALTH_CHECKER_THREAD_NAME, callback).start()
}

pub(crate) fn publish_version_event(
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

#[cfg(test)]
pub mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
    use crate::event::channel::pub_sub;
    use crate::event::SubAgentInternalEvent::AgentVersionInfo;
    use crate::sub_agent::version::version_checker::{
        spawn_version_checker, AgentVersion, VersionCheckError, VersionChecker,
    };
    use mockall::{mock, Sequence};
    use std::time::Duration;

    mock! {
        pub VersionCheckerMock {}
        impl VersionChecker for VersionCheckerMock {
            fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError>;
        }
    }

    #[test]
    fn test_spawn_version_checker() {
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionCheckerMock::new();
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
                Ok(AgentVersion::new(
                    "1.0.0".to_string(),
                    OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        let started_thread_context = spawn_version_checker(
            agent_id.clone(),
            version_checker,
            version_publisher,
            Duration::from_millis(10).into(),
        );

        // Check that we received the expected version event
        assert_eq!(
            AgentVersionInfo(AgentVersion {
                version: "1.0.0".to_string(),
                opamp_field: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            }),
            version_consumer.as_ref().recv().unwrap()
        );

        // Check that the thread is finished
        started_thread_context.stop().unwrap();

        // Check there are no more events
        assert!(version_consumer.as_ref().recv().is_err());
    }
}
