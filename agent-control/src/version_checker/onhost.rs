use std::process::Command;

use crate::agent_control::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
use crate::agent_type::runtime_config::on_host::executable::Args;
use crate::opamp::attributes::{Attribute, AttributeType, UpdateAttributesMessage};
use crate::version_checker::{
    AgentVersion, VersionCheckError, VersionChecker, publish_version_event,
};
use regex::Regex;

use crate::event::channel::EventPublisher;
use crate::sub_agent::identity::ID_ATTRIBUTE_NAME;
use std::fmt::Debug;
use tracing::{debug, info, info_span, warn};

pub struct OnHostAgentVersionChecker {
    pub(crate) path: String,
    pub(crate) args: Args,
    pub(crate) regex: Option<Regex>,
}

impl VersionChecker for OnHostAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        let output = Command::new(&self.path)
            .args(self.args.clone().into_vector())
            .output()
            .map_err(|e| VersionCheckError(format!("error executing version command: {e}")))?;
        let output = String::from_utf8_lossy(&output.stdout);

        let version = if let Some(regex) = &self.regex {
            let version_match = regex.find(&output).ok_or(VersionCheckError(
                "error checking agent version: version not found".to_string(),
            ))?;
            version_match.as_str().to_string()
        } else {
            output.to_string()
        };

        Ok(AgentVersion {
            version: version.as_str().to_string(),
            opamp_field: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        })
    }
}

pub(crate) fn check_version<V, T, F>(
    version_checker_id: String,
    version_checker: V,
    version_event_publisher: EventPublisher<T>,
    version_event_generator: F,
) where
    V: VersionChecker + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
    F: Fn(UpdateAttributesMessage) -> T + Send + Sync + 'static,
{
    let span = info_span!(
        "version_check",
        { ID_ATTRIBUTE_NAME } = %version_checker_id
    );
    let _guard = span.enter();

    debug!("starting to check version with the configured checker");

    match version_checker.check_agent_version() {
        Ok(agent_data) => {
            info!("agent version successfully checked");

            publish_version_event(
                &version_event_publisher,
                version_event_generator(vec![Attribute::from((
                    AttributeType::Identifying,
                    agent_data.opamp_field,
                    agent_data.version,
                ))]),
            );
        }
        Err(error) => {
            warn!("failed to check agent version: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::event::SubAgentInternalEvent;
    use crate::version_checker::tests::MockVersionChecker;
    use crate::{
        agent_control::defaults::OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY,
        event::channel::pub_sub,
    };

    use super::*;

    use rstest::rstest;

    #[rstest]
    #[cfg_attr(
        target_family = "unix",
        case::command_and_regex("echo", "Some data 1.0.0 Some more data", Some(r"\d+\.\d+\.\d+"))
    )]
    #[cfg_attr(target_family = "unix", case::command("echo", "-n 1.0.0", None))]
    #[cfg_attr(
        target_family = "windows",
        case::command_and_regex(
            "cmd",
            "/C echo Some data 1.0.0 Some more data",
            Some(r"\d+\.\d+\.\d+")
        )
    )]
    #[cfg_attr(
        target_family = "windows",
        case::command("cmd", "/C set /p=1.0.0<nul", None)
    )]
    fn test_check_agent_version(
        #[case] path: &str,
        #[case] args: String,
        #[case] regex: Option<&str>,
    ) {
        let agent_version = OnHostAgentVersionChecker {
            path: path.to_string(),
            args: Args(args),
            regex: regex.map(|r| Regex::new(r).unwrap()),
        }
        .check_agent_version()
        .unwrap();

        assert_eq!(agent_version.version.as_str(), "1.0.0");
        assert_eq!(
            agent_version.opamp_field.as_str(),
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
        );
    }

    #[test]
    fn test_check_version() {
        let (version_publisher, version_consumer) = pub_sub();

        let mut version_checker = MockVersionChecker::new();
        version_checker
            .expect_check_agent_version()
            .once()
            .returning(move || {
                Ok(AgentVersion {
                    version: "1.0.0".to_string(),
                    opamp_field: OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY.to_string(),
                })
            });

        check_version(
            AgentID::default().to_string(),
            version_checker,
            version_publisher,
            SubAgentInternalEvent::AgentAttributesUpdated,
        );

        // Check that we received the expected version event
        assert_eq!(
            SubAgentInternalEvent::AgentAttributesUpdated(vec![Attribute::from((
                AttributeType::Identifying,
                OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY,
                "1.0.0".to_string(),
            ))],),
            version_consumer.as_ref().recv().unwrap()
        );

        // Check there are no more events
        assert!(version_consumer.as_ref().recv().is_err());
    }
}
