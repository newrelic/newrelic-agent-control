use std::path::Path;
use std::{collections::HashMap, fmt::Display};

use std::ops::Deref;

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct AgentID(pub String);

impl Deref for AgentID {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for AgentID {
    fn as_ref(&self) -> &Path {
        // TODO: define how AgentID should be converted to a Path here.
        Path::new(&self.0)
    }
}

impl Display for AgentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
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

impl Deref for AgentTypeFQN {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
#[serde(deny_unknown_fields)]
pub struct AgentSupervisorConfig {
    pub agent_type: AgentTypeFQN,
    // FQN of the agent type, ex: newrelic/nrdot:0.1.0
    pub values_file: Option<String>, // path to the values file
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct OpAMPClientConfig {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod test {
    use super::*;

    impl AgentID {
        pub fn new(agent_id: &str) -> Self {
            Self(agent_id.to_string())
        }
    }

    const EXAMPLE_SUPERAGENT_CONFIG: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
agents:
  agent_1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const SUPERAGENT_CONFIG_UNKNOWN_FIELDS: &str = r#"
# opamp:
# agents:
random_field: random_value
"#;

    const SUPERAGENT_CONFIG_UNKNOWN_OPAMP_FIELDS: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  some-key: some-value
agents:
  agent_1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const SUPERAGENT_CONFIG_UNKNOWN_AGENT_FIELDS: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  some-key: some-value
agents:
  agent_1:
    agent_type: namespace/agent_type:0.0.1
    agent_random: true
"#;

    #[test]
    fn basic_parse() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_SUPERAGENT_CONFIG);
        assert!(actual.is_ok());
    }

    #[test]
    fn parse_with_unknown_fields() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_UNKNOWN_FIELDS);
        assert!(actual.is_err());
        let actual =
            serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_UNKNOWN_OPAMP_FIELDS);
        assert!(actual.is_err());
        let actual =
            serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_UNKNOWN_AGENT_FIELDS);
        assert!(actual.is_err());
    }

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
