use std::{collections::HashMap, path::PathBuf};

use super::variable::{Variable, namespace::Namespace};

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent Agent ID
    pub agent_id: String,
    pub auto_generated_dir: PathBuf,
}

impl AgentAttributes {
    const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";
    const GENERATED_DIR: &'static str = "generated_dir";

    /// returns the variables from the sub-agent attributes source 'nr-sub'.
    pub fn sub_agent_variables(&self) -> HashMap<String, Variable> {
        let auto_generated_subagent_dir = self
            .auto_generated_dir
            .join(&self.agent_id)
            .to_string_lossy()
            .to_string();
        HashMap::from([
            (
                Namespace::SubAgent.namespaced_name(Self::VARIABLE_SUB_AGENT_ID),
                Variable::new_final_string_variable(self.agent_id.clone()),
            ),
            (
                Namespace::SubAgent.namespaced_name(Self::GENERATED_DIR),
                Variable::new_final_string_variable(auto_generated_subagent_dir),
            ),
        ])
    }
}
