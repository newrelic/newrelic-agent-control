use std::path::Path;
use std::{collections::HashMap, fmt::Display};

use std::ops::Deref;

use crate::config::error::SuperAgentConfigError;
use crate::super_agent::defaults::SUPER_AGENT_ID;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Deserialize, PartialEq, Clone, Hash, Eq)]
#[serde(try_from = "String")]
pub struct AgentID(String);

#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("AgentID allows only a-zA-Z0-9_-")]
    InvalidAgentID,
    #[error("AgentID '{0}' is reserved")]
    InvalidAgentIDUsesReservedOne(String),
}

impl TryFrom<String> for AgentID {
    type Error = AgentTypeError;
    fn try_from(str: String) -> Result<Self, Self::Error> {
        if str.eq(SUPER_AGENT_ID) {
            return Err(AgentTypeError::InvalidAgentIDUsesReservedOne(
                SUPER_AGENT_ID.to_string(),
            ));
        }

        if str
            .chars()
            .all(|x| x.is_alphanumeric() || x.eq(&'_') || x.eq(&'-'))
        {
            Ok(AgentID(str))
        } else {
            Err(AgentTypeError::InvalidAgentID)
        }
    }
}

impl AgentID {
    pub fn new(str: &str) -> Result<Self, AgentTypeError> {
        Self::try_from(str.to_string())
    }
    // For super agent ID we need to skip validation
    pub fn new_super_agent_id() -> Self {
        Self(SUPER_AGENT_ID.to_string())
    }
    pub fn get(&self) -> String {
        String::from(&self.0)
    }
}

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

/// SubAgentsConfig represents the configuration for the sub agents.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub struct SubAgentsConfig(pub(crate) HashMap<AgentID, SubAgentConfig>);

impl Deref for SubAgentsConfig {
    type Target = HashMap<AgentID, SubAgentConfig>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<HashMap<AgentID, SubAgentConfig>> for SubAgentsConfig {
    fn from(value: HashMap<AgentID, SubAgentConfig>) -> Self {
        Self(value)
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SuperAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    #[serde(default)]
    pub agents: SubAgentsConfig,

    /// opamp contains the OpAMP client configuration
    pub opamp: Option<OpAMPClientConfig>,
}

impl SuperAgentConfig {
    pub fn sub_agent_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<&SubAgentConfig, SuperAgentConfigError> {
        self.agents
            .get(agent_id)
            .ok_or(SuperAgentConfigError::SubAgentNotFound(
                agent_id.to_string(),
            ))
    }
}

#[derive(Debug, PartialEq, Deserialize, Clone)]
pub struct AgentTypeFQN(pub String);

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
pub struct SubAgentConfig {
    pub agent_type: AgentTypeFQN, // FQN of the agent type, ex: newrelic/nrdot:0.1.0
}

pub fn get_values_file_path(agent_id: &AgentID) -> String {
    format!("/etc/newrelic-super-agent/agents.d/{}/values.yml", agent_id)
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

    const SUPERAGENT_CONFIG_WRONG_AGENT_ID: &str = r#"
agents:
  agent/1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const SUPERAGENT_CONFIG_RESERVED_AGENT_ID: &str = r#"
agents:
  super-agent:
    agent_type: namespace/agent_type:0.0.1
"#;

    #[test]
    fn agent_id_validator() {
        assert!(AgentID::try_from("abc012_-".to_string()).is_ok());
        assert!(AgentID::try_from("ab".to_string()).is_ok());
        assert!(AgentID::try_from("01".to_string()).is_ok());
        assert!(AgentID::try_from("-".to_string()).is_ok());
        assert!(AgentID::try_from("abc012/".to_string()).is_err());
        assert!(AgentID::try_from("abc012.".to_string()).is_err());
    }

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
    fn parse_with_wrong_agent_id() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_WRONG_AGENT_ID);
        assert!(actual.is_err());
        assert_eq!(
            actual.unwrap_err().to_string(),
            "agents: AgentID allows only a-zA-Z0-9_- at line 3 column 3"
        )
    }

    #[test]
    fn parse_with_reserved_agent_id() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_RESERVED_AGENT_ID);
        assert!(actual.is_err());
        assert_eq!(
            actual.unwrap_err().to_string(),
            "agents: AgentID 'super-agent' is reserved at line 3 column 3"
        )
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
