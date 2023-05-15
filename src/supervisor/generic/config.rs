use std::collections::HashMap;
use serde::Deserialize;

/// The Config for the meta-agent and the managers
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct GenericSupervisorConfig {
    pub(crate) op_amp: String,
    pub(crate) agents: HashMap<String, AgentConf>,
}

#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct Executable {
    pub binary: String,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct AgentConf {
    pub agent_name: String,
    pub agent_type: String,
    pub executables: Vec<Executable>,
}