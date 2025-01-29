use crate::agent_control::config::AgentID;
use crate::agent_type::version_config::VersionCheckerInterval;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::SubAgentInternalEvent;
use std::thread;
use tracing::{debug, error};

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
    cancel_signal: EventConsumer<CancellationMessage>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    interval: VersionCheckerInterval,
) where
    V: VersionChecker + Send + Sync + 'static,
{
    thread::Builder::new()
        .name("Version checker ".to_string())
        .spawn(move || loop {
            debug!(%agent_id, "starting to check version with the configured checker");

            match version_checker.check_agent_version() {
                Ok(agent_data) => {
                    publish_version_event(
                        &sub_agent_internal_publisher,
                        SubAgentInternalEvent::AgentVersionInfo(agent_data),
                    );
                }
                Err(error) => {
                    error!(%agent_id, %error, "failed to check agent version");
                }
            }

            if cancel_signal.is_cancelled(interval.into()) {
                break;
            }
        })
        .expect("thread config should be valid");
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
    use crate::agent_control::config::AgentID;
    use crate::agent_control::defaults::OPAMP_CHART_VERSION_ATTRIBUTE_KEY;
    use crate::event::channel::pub_sub;
    use crate::event::SubAgentInternalEvent;
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
        let (cancel_publisher, cancel_signal) = pub_sub();
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionCheckerMock::new();
        let mut seq = Sequence::new();
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

        version_checker
            .expect_check_agent_version()
            .once()
            .in_sequence(&mut seq)
            .returning(move || {
                cancel_publisher.publish(()).unwrap();
                Err(VersionCheckError::Generic(
                    "mocked version check error!".to_string(),
                ))
            });

        let agent_id = AgentID::new("test-agent").unwrap();
        spawn_version_checker(
            agent_id,
            version_checker,
            cancel_signal,
            version_publisher,
            Duration::default().into(),
        );

        let expected_version_events: Vec<SubAgentInternalEvent> = {
            vec![AgentVersionInfo(AgentVersion {
                version: "1.0.0".to_string(),
                opamp_field: OPAMP_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
            })]
        };
        let actual_version_events = version_consumer.as_ref().iter().collect::<Vec<_>>();
        assert_eq!(expected_version_events, actual_version_events);
    }
}
