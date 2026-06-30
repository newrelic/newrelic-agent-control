//! Sub-agent attributes used to build the reserved variables that template an agent type.
use super::variable::{Variable, namespace::Namespace};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{AGENT_FILESYSTEM_FOLDER_NAME, SHARED_FILESYSTEM_FOLDER_NAME};
use std::{collections::HashMap, path::PathBuf};
use thiserror::Error;
use tracing::debug;

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent Agent ID
    agent_id: String,
    agent_filesystem_dir: PathBuf,
    shared_filesystem_dir: PathBuf,
    remote_dir: PathBuf,
}

/// Error returned when [`AgentAttributes`] cannot be created.
#[derive(Debug, Error)]
#[error("Failed to create AgentAttributes: {0}")]
pub struct AgentAttributesCreateError(String);

impl AgentAttributes {
    /// Variable name holding the sub-agent id.
    pub const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";
    /// Variable name holding the sub-agent's dedicated filesystem directory.
    pub const VARIABLE_FILESYSTEM_AGENT_DIR: &'static str = "filesystem_agent_dir";
    /// Variable name holding the filesystem directory shared across sub-agents.
    pub const VARIABLE_SHARED_FILESYSTEM_DIR: &'static str = "shared_filesystem_dir";
    /// Variable name holding the sub-agent's remote directory.
    pub const VARIABLE_REMOTE_DIR: &'static str = "remote_dir";

    /// Builds [`AgentAttributes`] for a sub-agent. Returns an error if the given id is a reserved
    /// (non sub-agent) id.
    pub fn try_new(
        agent_id: AgentID,
        remote_dir: PathBuf,
    ) -> Result<Self, AgentAttributesCreateError> {
        if let AgentID::SubAgent(agent_id) = agent_id {
            let agent_filesystem_dir = remote_dir
                .join(AGENT_FILESYSTEM_FOLDER_NAME)
                .join(&agent_id);
            // Shared across sub-agents, so it is not suffixed with the agent id.
            let shared_filesystem_dir = remote_dir.join(SHARED_FILESYSTEM_FOLDER_NAME);
            debug!(id = %agent_id, "filesystem directory path set to {}", agent_filesystem_dir.display());
            Ok(Self {
                agent_id: agent_id.to_string(),
                agent_filesystem_dir,
                shared_filesystem_dir,
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
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_SHARED_FILESYSTEM_DIR),
                Variable::new_final_string_variable(self.shared_filesystem_dir.to_string_lossy()),
            ),
            (
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_REMOTE_DIR),
                Variable::new_final_string_variable(self.remote_dir.to_string_lossy()),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::trivial_value::TrivialValue;

    fn final_string(vars: &HashMap<String, Variable>, name: &str) -> String {
        let key = Namespace::SubAgent.namespaced_name(name);
        match vars
            .get(&key)
            .and_then(Variable::get_final_value)
            .unwrap_or_else(|| panic!("missing variable {key}"))
        {
            TrivialValue::String(s) => s,
            other => panic!("expected string for {key}, got {other:?}"),
        }
    }

    #[test]
    fn filesystems_are_available() {
        let remote_dir = PathBuf::from("/var/lib/newrelic-agent-control");
        let agent_id = AgentID::try_from("my-agent").unwrap();
        let attrs = AgentAttributes::try_new(agent_id, remote_dir).unwrap();

        let vars = attrs.sub_agent_variables();

        // Shared dir lives directly under the remote dir, with no agent-id suffix.
        assert_eq!(
            final_string(&vars, AgentAttributes::VARIABLE_SHARED_FILESYSTEM_DIR),
            "/var/lib/newrelic-agent-control/shared-filesystem",
        );
        // The per-agent dir, in contrast, is suffixed with the agent id.
        assert_eq!(
            final_string(&vars, AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR),
            "/var/lib/newrelic-agent-control/filesystem/my-agent",
        );
    }

    #[test]
    fn shared_filesystem_dir_is_identical_across_agents() {
        let remote_dir = PathBuf::from("/var/lib/newrelic-agent-control");
        let a = AgentAttributes::try_new(AgentID::try_from("agent-a").unwrap(), remote_dir.clone())
            .unwrap();
        let b =
            AgentAttributes::try_new(AgentID::try_from("agent-b").unwrap(), remote_dir).unwrap();

        let key = AgentAttributes::VARIABLE_SHARED_FILESYSTEM_DIR;
        assert_eq!(
            final_string(&a.sub_agent_variables(), key),
            final_string(&b.sub_agent_variables(), key),
        );
    }
}
