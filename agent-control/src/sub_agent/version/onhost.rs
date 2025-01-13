use crate::agent_control::config::AgentTypeFQN;
use crate::agent_control::defaults::{
    FQN_NAME_INFRA_AGENT, FQN_NAME_NRDOT, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
};
use crate::sub_agent::version::version_checker::{AgentVersion, VersionCheckError, VersionChecker};
use tracing::error;

const NEWRELIC_INFRA_AGENT_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NEWRELIC_INFRA_AGENT_VERSION"), "0.0.0");
const NR_OTEL_COLLECTOR_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NR_OTEL_COLLECTOR_VERSION"), "0.0.0");

pub struct OnHostAgentVersionChecker {
    agent_version: AgentVersion,
}

impl OnHostAgentVersionChecker {
    pub fn checked_new(agent_type_fqn: AgentTypeFQN) -> Option<Self> {
        match retrieve_version(&agent_type_fqn) {
            Ok(agent_version) => Some(Self { agent_version }),
            Err(e) => {
                error!("error checking agent version: {}", e);
                None
            }
        }
    }
}

impl VersionChecker for OnHostAgentVersionChecker {
    fn check_agent_version(&self) -> Result<AgentVersion, VersionCheckError> {
        error!("DEBUG VERSION RETRIEVED!");
        Ok(self.agent_version.clone())
    }
}

fn retrieve_version(agent_type_fqn: &AgentTypeFQN) -> Result<AgentVersion, VersionCheckError> {
    match agent_type_fqn.name().as_str() {
        FQN_NAME_INFRA_AGENT => Ok(AgentVersion::new(
            NEWRELIC_INFRA_AGENT_VERSION.to_string(),
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        )),
        FQN_NAME_NRDOT => Ok(AgentVersion::new(
            NR_OTEL_COLLECTOR_VERSION.to_string(),
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        )),
        _ => Err(VersionCheckError::Generic(format!(
            "no match found for agent type: {}",
            agent_type_fqn
        ))),
    }
}

pub fn onhost_sub_agent_versions() -> String {
    format!(
        r#"New Relic Sub Agent Versions:
    {FQN_NAME_INFRA_AGENT} : {NEWRELIC_INFRA_AGENT_VERSION}
    {FQN_NAME_NRDOT} : {NR_OTEL_COLLECTOR_VERSION}"#
    )
}
