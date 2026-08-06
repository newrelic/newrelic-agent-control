//! Sub-agent attributes used to build the reserved variables that template an agent type.
use super::variable::{
    Variable,
    namespace::{Namespace, VariableName},
};
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
    /// Variable name holding the sub-agent id under nr-sub namespace.
    pub const NR_SUB_AGENT_ID: &'static str = "agent_id";
    /// Variable name holding the sub-agent's dedicated filesystem directory under nr-sub namespace.
    pub const NR_SUB_FILESYSTEM_AGENT_DIR: &'static str = "filesystem_agent_dir";
    /// Variable name holding the filesystem directory shared across sub-agents under ns-sub namespace.
    pub const NR_SUB_SHARED_FILESYSTEM_DIR: &'static str = "shared_filesystem_dir";
    /// Variable name holding the sub-agent's remote directory under nr-sub namespace.
    pub const NR_SUB_REMOTE_DIR: &'static str = "remote_dir";

    /// Variable name holding the sub-agent's dedicated filesystem directory under nr-path namespace.
    pub const NR_PATH_AGENT_DIR: &'static str = "agent_dir";

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

    /// Returns the variables from the sub-agent attributes source 'nr-sub'.
    pub fn nr_sub_variables(&self) -> HashMap<VariableName, Variable> {
        HashMap::from([
            (
                VariableName::new(Namespace::SubAgent, Self::NR_SUB_AGENT_ID),
                Variable::new_final_string_variable(&self.agent_id),
            ),
            (
                VariableName::new(Namespace::SubAgent, Self::NR_SUB_FILESYSTEM_AGENT_DIR),
                Variable::new_final_string_variable(self.agent_filesystem_dir.to_string_lossy()),
            ),
            (
                VariableName::new(Namespace::SubAgent, Self::NR_SUB_SHARED_FILESYSTEM_DIR),
                Variable::new_final_string_variable(self.shared_filesystem_dir.to_string_lossy()),
            ),
            (
                VariableName::new(Namespace::SubAgent, Self::NR_SUB_REMOTE_DIR),
                Variable::new_final_string_variable(self.remote_dir.to_string_lossy()),
            ),
        ])
    }

    /// Returns the variables from agent attributes to be exposed as `nr-path`
    pub fn nr_path_variables(&self) -> HashMap<VariableName, Variable> {
        HashMap::from([(
            VariableName::new(Namespace::Path, Self::NR_PATH_AGENT_DIR),
            Variable::new_final_string_variable(self.agent_filesystem_dir.to_string_lossy()),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::defaults::AGENT_CONTROL_DATA_DIR;
    use crate::agent_type::trivial_value::TrivialValue;

    fn final_string(
        vars: &HashMap<VariableName, Variable>,
        namespace: Namespace,
        name: &str,
    ) -> String {
        let key = VariableName::new(namespace, name);
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
        let remote_dir = PathBuf::from(AGENT_CONTROL_DATA_DIR);
        let agent_id = AgentID::try_from("my-agent").unwrap();
        let attributes = AgentAttributes::try_new(agent_id, remote_dir.clone()).unwrap();

        let nr_sub_vars = attributes.nr_sub_variables();
        // Build expected paths via `join` so separators match the platform (e.g. `\` on Windows).
        // Shared dir lives directly under the remote dir, with no agent-id suffix.
        let expected_shared = remote_dir
            .join("shared-filesystem")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            final_string(
                &nr_sub_vars,
                Namespace::SubAgent,
                AgentAttributes::NR_SUB_SHARED_FILESYSTEM_DIR
            ),
            expected_shared,
        );
        // The per-agent dir, in contrast, is suffixed with the agent id.
        let expected_agent = remote_dir
            .join("filesystem")
            .join("my-agent")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            final_string(
                &nr_sub_vars,
                Namespace::SubAgent,
                AgentAttributes::NR_SUB_FILESYSTEM_AGENT_DIR
            ),
            expected_agent,
        );

        let nr_paths_vars = attributes.nr_path_variables();
        assert_eq!(
            final_string(
                &nr_paths_vars,
                Namespace::Path,
                AgentAttributes::NR_PATH_AGENT_DIR
            ),
            expected_agent,
        );
    }

    #[test]
    fn shared_filesystem_dir_is_identical_across_agents() {
        let remote_dir = PathBuf::from(AGENT_CONTROL_DATA_DIR);
        let a = AgentAttributes::try_new(AgentID::try_from("agent-a").unwrap(), remote_dir.clone())
            .unwrap()
            .nr_sub_variables();
        let b = AgentAttributes::try_new(AgentID::try_from("agent-b").unwrap(), remote_dir)
            .unwrap()
            .nr_sub_variables();

        let key = AgentAttributes::NR_SUB_SHARED_FILESYSTEM_DIR;
        assert_eq!(
            final_string(&a, Namespace::SubAgent, key),
            final_string(&b, Namespace::SubAgent, key),
        );
    }
}
