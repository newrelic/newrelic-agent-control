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

#[derive(Error, Debug)]
pub enum AgentControlError {
    #[error("could not resolve config: {0}")]
    ConfigResolve(#[from] AgentControlConfigError),

    #[error("{0}")]
    OpAMPClient(#[from] ClientError),

    #[error("{0}")]
    OpAMPStartedClient(#[from] StartedClientError),

    #[error("{0}")]
    SubAgentBuilder(#[from] SubAgentBuilderError),

    #[error("{0}")]
    SubAgentCollection(#[from] SubAgentCollectionError),

    #[error("remote config error: {0}")]
    RemoteConfig(#[from] OpampRemoteConfigError),

    #[error("parsing remote config into YAMLConfig: {0}")]
    YAMLConfig(#[from] YAMLConfigError),

    #[error("agent control remote config validation error: {0}")]
    RemoteConfigValidator(String),

    #[error("resource cleaner error: {0}")]
    ResourceCleaner(#[from] ResourceCleanerError),

    #[error("updater error: {0}")]
    Updater(#[from] UpdaterError),

    #[error("failed to build agents: {0}")]
    BuildingSubagents(BuildingSubagentErrors),
}

#[derive(Debug, Default)]
pub struct BuildingSubagentErrors(Vec<(AgentID, AgentControlError)>);
impl BuildingSubagentErrors {
    pub fn push(&mut self, agent_id: AgentID, error: AgentControlError) {
        self.0.push((agent_id, error));
    }
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
