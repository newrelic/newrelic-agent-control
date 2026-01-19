use super::variable::{Variable, namespace::Namespace};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use std::{collections::HashMap, path::PathBuf};
use thiserror::Error;
use tracing::debug;

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent Agent ID
    agent_id: String,
    agent_filesystem_dir: PathBuf,
    remote_dir: PathBuf,
}

#[derive(Debug, Error)]
#[error("Failed to create AgentAttributes: {0}")]
pub struct AgentAttributesCreateError(String);

impl AgentAttributes {
    pub const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";
    pub const VARIABLE_FILESYSTEM_AGENT_DIR: &'static str = "filesystem_agent_dir";
    pub const VARIABLE_REMOTE_DIR: &'static str = "remote_dir";

    pub fn try_new(
        agent_id: AgentID,
        remote_dir: PathBuf,
    ) -> Result<Self, AgentAttributesCreateError> {
        if let AgentID::SubAgent(agent_id) = agent_id {
            let agent_filesystem_dir = remote_dir
                .join(AGENT_FILESYSTEM_FOLDER_NAME)
                .join(&agent_id);
            debug!(id = %agent_id, "filesystem directory path set to {}", agent_filesystem_dir.display());
            Ok(Self {
                agent_id: agent_id.to_string(),
                agent_filesystem_dir,
                remote_dir,
            })
        } else {
            Err(AgentAttributesCreateError("Used reserved Agent ID".into()))
        }
    }

    /// returns the variables from the sub-agent attributes source 'nr-sub'.
    pub fn sub_agent_variables(&self) -> HashMap<String, Variable> {
        HashMap::from([
            (
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_SUB_AGENT_ID),
                Variable::new_final_string_variable(&self.agent_id),
            ),
            (
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(self.agent_filesystem_dir.to_string_lossy()),
            ),
            (
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_REMOTE_DIR),
                Variable::new_final_string_variable(self.remote_dir.to_string_lossy()),
            ),
        ])
    }
}
