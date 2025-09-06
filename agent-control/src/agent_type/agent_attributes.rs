use std::{collections::HashMap, path::PathBuf};

use thiserror::Error;
use tracing::debug;

use crate::agent_control::agent_id::AgentID;

use super::variable::{Variable, namespace::Namespace};

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent Agent ID
    agent_id: String,
    auto_generated_dir: PathBuf,
}

#[derive(Debug, Error)]
#[error("Failed to create AgentAttributes: {0}")]
pub struct AgentAttributesCreateError(String);

impl AgentAttributes {
    pub const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";
    pub const GENERATED_DIR: &'static str = "agent_dir";

    pub fn try_new(
        agent_id: AgentID,
        auto_generated_dir: PathBuf,
    ) -> Result<Self, AgentAttributesCreateError> {
        if let AgentID::SubAgent(agent_id) = agent_id {
            let auto_generated_dir = auto_generated_dir.join(&agent_id);
            debug!(id = %agent_id, "auto-generated directory path set to {}", auto_generated_dir.display());
            Ok(Self {
                agent_id: agent_id.to_string(),
                auto_generated_dir,
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
                Namespace::SubAgent.namespaced_name(Self::GENERATED_DIR),
                Variable::new_final_string_variable(self.auto_generated_dir.to_string_lossy()),
            ),
        ])
    }
}
