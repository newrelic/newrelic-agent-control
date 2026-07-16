//! Errors produced by the Agent Control supervisor runtime.

use super::config::AgentControlConfigError;
use super::resource_cleaner::ResourceCleanerError;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::version_updater::updater::UpdaterError;

use crate::opamp::remote_config::OpampRemoteConfigError;
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentCollectionError};
use crate::values::yaml_config::YAMLConfigError;
use opamp_client::{ClientError, StartedClientError};
use std::fmt::{Debug, Display};
use thiserror::Error;

/// Errors that can occur while running the Agent Control supervisor.
#[derive(Error, Debug)]
pub enum AgentControlError {
    /// The configuration could not be resolved.
    #[error("could not resolve config: {0}")]
    ConfigResolve(#[from] AgentControlConfigError),

    /// Error from the (not-yet-started) OpAMP client.
    #[error("{0}")]
    OpAMPClient(#[from] ClientError),

    /// Error from the started OpAMP client.
    #[error("{0}")]
    OpAMPStartedClient(#[from] StartedClientError),

    /// Error building a sub-agent.
    #[error("{0}")]
    SubAgentBuilder(#[from] SubAgentBuilderError),

    /// Error operating on the sub-agent collection.
    #[error("{0}")]
    SubAgentCollection(#[from] SubAgentCollectionError),

    /// Error handling an OpAMP remote configuration.
    #[error("remote config error: {0}")]
    RemoteConfig(#[from] OpampRemoteConfigError),

    /// Error parsing a remote configuration into a YAML config.
    #[error("parsing remote config into YAMLConfig: {0}")]
    YAMLConfig(#[from] YAMLConfigError),

    /// Remote configuration failed validation.
    #[error("agent control remote config validation error: {0}")]
    RemoteConfigValidator(String),

    /// Error cleaning up resources.
    #[error("resource cleaner error: {0}")]
    ResourceCleaner(#[from] ResourceCleanerError),

    /// Error from the version updater.
    #[error("updater error: {0}")]
    Updater(#[from] UpdaterError),

    /// One or more sub-agents failed to build/apply.
    #[error("failed to build agents: {0}")]
    BuildingSubagents(BuildingSubagentErrors),
}

impl AgentControlError {
    /// Returns a stable, low-cardinality error code suitable for metric labels.
    pub fn error_code(&self) -> &'static str {
        match self {
            AgentControlError::ConfigResolve(_) => "config_resolve",
            AgentControlError::OpAMPClient(_) => "opamp_client",
            AgentControlError::OpAMPStartedClient(_) => "opamp_started_client",
            AgentControlError::SubAgentBuilder(_) => "sub_agent_builder",
            AgentControlError::SubAgentCollection(_) => "sub_agent_collection",
            AgentControlError::RemoteConfig(_) => "remote_config",
            AgentControlError::YAMLConfig(_) => "yaml_config",
            AgentControlError::RemoteConfigValidator(_) => "remote_config_validator",
            AgentControlError::ResourceCleaner(_) => "resource_cleaner",
            AgentControlError::Updater(_) => "updater",
            AgentControlError::BuildingSubagents(_) => "building_subagents",
        }
    }
}

/// Accumulates per-agent errors collected while building or applying sub-agents.
#[derive(Debug, Default)]
pub struct BuildingSubagentErrors(Vec<(AgentID, AgentControlError)>);
impl BuildingSubagentErrors {
    /// Records an error for the given agent.
    pub fn push(&mut self, agent_id: AgentID, error: AgentControlError) {
        self.0.push((agent_id, error));
    }
    /// Returns `true` when no errors have been recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Display for BuildingSubagentErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let errors = self
            .0
            .iter()
            .map(|(agent_id, error)| format!("agent_id: {agent_id}, error: {error}"))
            .reduce(|acc, s| format!("{acc}, {s}"))
            .unwrap_or_default();
        write!(f, "[{errors}]")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_is_stable_and_low_cardinality() {
        assert_eq!(
            AgentControlError::RemoteConfigValidator("bad config".to_string()).error_code(),
            "remote_config_validator"
        );
        assert_eq!(
            AgentControlError::BuildingSubagents(BuildingSubagentErrors::default()).error_code(),
            "building_subagents"
        );
    }
}
