use std::{collections::HashMap, fmt::Display};

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct AgentID(pub String);

impl AgentID {
    pub fn get(&self) -> String {
        String::from(&self.0)
    }
}

impl Display for AgentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SuperAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    pub agents: HashMap<AgentID, AgentSupervisorConfig>,

    /// opamp contains the OpAMP client configuration
    pub opamp: Option<OpAMPClientConfig>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AgentSupervisorConfig {
    pub agent_type: String,  // FQN of the agent type, ex: newrelic/nrdot:0.1.0
    pub values_file: String, // path to the values file
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
pub struct OpAMPClientConfig {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
}
