use std::collections::HashMap;

use super::variable::{Variable, namespace::Namespace};

/// contains any attribute from the sub-agent that is used to build or modify variables used to template the AgentType.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentAttributes {
    /// sub-agent Agent ID
    pub agent_id: String,
}

impl AgentAttributes {
    const VARIABLE_SUB_AGENT_ID: &'static str = "agent_id";

    /// returns the variables from the sub-agent attributes source 'nr-sub'.
    pub fn sub_agent_variables(&self) -> HashMap<String, Variable> {
        HashMap::from([(
            Namespace::SubAgent.namespaced_name(Self::VARIABLE_SUB_AGENT_ID),
            Variable::new_final_string_variable(self.agent_id.clone()),
        )])
    }
}
