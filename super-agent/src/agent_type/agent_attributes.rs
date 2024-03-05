use std::{collections::HashMap, path::PathBuf};

use super::variable::{definition::VariableDefinition, namespace::Namespace};

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent generated config path
    pub generated_configs_path: PathBuf,
    /// sub-agent Agent ID
    pub agent_id: String,
}

impl AgentAttributes {
    const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";

    /// returns the variables from the sub-agent attributes source 'nr-sub'.
    pub fn sub_agent_variables(&self) -> HashMap<String, VariableDefinition> {
        HashMap::from([(
            Namespace::SubAgent.namespaced_name(Self::VARIABLE_SUB_AGENT_ID),
            VariableDefinition::new_sub_agent_string_variable(self.agent_id.clone()),
        )])
    }

    /// extends the path of all variables that have a kind with path, with the sub agent generated config path.
    pub fn extend_file_paths(
        &self,
        mut variables: HashMap<String, VariableDefinition>,
    ) -> HashMap<String, VariableDefinition> {
        variables
            .values_mut()
            .for_each(|v| v.extend_file_path(self.generated_configs_path.as_path()));
        variables
    }
}
