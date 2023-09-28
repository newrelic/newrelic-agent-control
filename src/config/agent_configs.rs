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
    #[serde(default)]
    pub agents: HashMap<AgentID, AgentSupervisorConfig>,

    /// opamp contains the OpAMP client configuration
    pub opamp: Option<OpAMPClientConfig>,
}

#[derive(Debug, PartialEq, Deserialize, Clone)]
pub struct AgentTypeFQN(String);

impl AgentTypeFQN {
    pub fn namespace(&self) -> String {
        self.0.chars().take_while(|&i| i != '/').collect()
    }

    pub fn name(&self) -> String {
        self.0
            .chars()
            .skip_while(|&i| i != '/')
            .skip(1)
            .take_while(|&i| i != ':')
            .collect()
    }

    pub fn version(&self) -> String {
        self.0.chars().skip_while(|&i| i != ':').skip(1).collect()
    }
}

impl Display for AgentTypeFQN {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<&str> for AgentTypeFQN {
    fn from(value: &str) -> Self {
        AgentTypeFQN(value.to_string())
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AgentSupervisorConfig {
    pub agent_type: AgentTypeFQN, // FQN of the agent type, ex: newrelic/nrdot:0.1.0
    pub values_file: Option<String>, // path to the values file
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
pub struct OpAMPClientConfig {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_agent_type_fqn() {
        let fqn: AgentTypeFQN = "newrelic/nrdot:0.1.0".into();
        assert_eq!(fqn.namespace(), "newrelic");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "0.1.0");
    }

    #[test]
    fn bad_agent_type_fqn_no_version() {
        let fqn: AgentTypeFQN = "newrelic/nrdot".into();
        assert_eq!(fqn.namespace(), "newrelic");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "");

        let fqn: AgentTypeFQN = "newrelic/nrdot:".into();
        assert_eq!(fqn.namespace(), "newrelic");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "");
    }

    #[test]
    fn bad_agent_type_fqn_no_name() {
        let fqn: AgentTypeFQN = "newrelic/:0.1.0".into();
        assert_eq!(fqn.namespace(), "newrelic");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "0.1.0");
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace() {
        let fqn: AgentTypeFQN = "/nrdot:0.1.0".into();
        assert_eq!(fqn.namespace(), "");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "0.1.0");
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace_no_version() {
        let fqn: AgentTypeFQN = "/nrdot".into();
        assert_eq!(fqn.namespace(), "");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "");
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace_no_name() {
        let fqn: AgentTypeFQN = "/:0.1.0".into();
        assert_eq!(fqn.namespace(), "");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "0.1.0");
    }

    #[test]
    fn bad_agent_type_fqn_namespace_separator() {
        let fqn: AgentTypeFQN = "/".into();
        assert_eq!(fqn.namespace(), "");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "");
    }

    #[test]
    fn bad_agent_type_fqn_empty_string() {
        let fqn: AgentTypeFQN = "".into();
        assert_eq!(fqn.namespace(), "");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "");
    }

    #[test]
    fn bad_agent_type_fqn_only_version_separator() {
        let fqn: AgentTypeFQN = ":".into();
        assert_eq!(fqn.namespace(), ":");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "");
    }

    #[test]
    fn bad_agent_type_fqn_only_word() {
        let fqn: AgentTypeFQN = "only_namespace".into();
        assert_eq!(fqn.namespace(), "only_namespace");
        assert_eq!(fqn.name(), "");
        assert_eq!(fqn.version(), "");
    }
}
