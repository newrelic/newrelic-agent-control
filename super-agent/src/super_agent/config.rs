use crate::logging::config::LoggingConfig;
use crate::opamp::remote_config::RemoteConfigError;
use crate::super_agent::config_storer::file::ConfigStoreError;
use crate::super_agent::defaults::{default_capabilities, SUPER_AGENT_ID};
use crate::super_agent::http_server::config::ServerConfig;
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
use kube::core::TypeMeta;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::path::Path;
use std::{collections::HashMap, fmt::Display};
use thiserror::Error;

const AGENT_ID_MAX_LENGTH: usize = 32;

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Hash, Eq)]
#[serde(try_from = "String")]
pub struct AgentID(String);

#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("AgentID must contain 32 characters at most, contain alphanumeric characters or dashes only, start with alphabetic, and end with alphanumeric")]
    InvalidAgentID,
    #[error("AgentID '{0}' is reserved")]
    InvalidAgentIDUsesReservedOne(String),
    #[error("AgentType must have a valid namespace")]
    InvalidAgentTypeNamespace,
    #[error("AgentType must have a valid name")]
    InvalidAgentTypeName,
    #[error("AgentType must have a valid version")]
    InvalidAgentTypeVersion,
}

#[derive(Error, Debug)]
pub enum SuperAgentConfigError {
    #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
    #[error("error from k8s storer loading SAConfig: {0}")]
    FailedToPersistK8s(#[from] crate::k8s::Error),

    #[error("error loading the super agent config: `{0}`")]
    LoadConfigError(#[from] ConfigStoreError),

    #[error("sub agent configuration `{0}` not found")]
    SubAgentNotFound(String),

    #[error("configuration is not valid YAML: `{0}`")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),

    #[error("remote config error: `{0}`")]
    RemoteConfigError(#[from] RemoteConfigError),

    #[error("remote config error: `{0}`")]
    IOError(#[from] std::io::Error),
}

impl TryFrom<String> for AgentID {
    type Error = AgentTypeError;
    fn try_from(str: String) -> Result<Self, Self::Error> {
        if str.eq(SUPER_AGENT_ID) {
            return Err(AgentTypeError::InvalidAgentIDUsesReservedOne(
                SUPER_AGENT_ID.to_string(),
            ));
        }

        if AgentID::check_string(&str) {
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
    pub fn is_super_agent_id(&self) -> bool {
        self.0.eq(SUPER_AGENT_ID)
    }
    /// Checks if a string reference has valid format to build an [AgentID].
    /// It follows [RFC 1035 Label names](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#rfc-1035-label-names),
    /// and sets a shorter maximum length to avoid issues when the agent-id is used to compose names.
    fn check_string(s: &str) -> bool {
        s.len() <= AGENT_ID_MAX_LENGTH
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.ends_with(|c: char| c.is_ascii_alphanumeric())
            && s.chars()
                .all(|c| c.eq(&'-') || c.is_ascii_digit() || c.is_ascii_lowercase())
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

/// SuperAgentDynamicConfig represents the dynamic part of the superAgent config.
/// The dynamic configuration can be changed remotely.
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct SuperAgentDynamicConfig {
    pub agents: SubAgentsMap,
}

pub type SubAgentsMap = HashMap<AgentID, SubAgentConfig>;

impl TryFrom<&str> for SuperAgentDynamicConfig {
    type Error = SuperAgentConfigError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(serde_yaml::from_str(value)?)
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SuperAgentConfig {
    #[serde(default)]
    pub log: LoggingConfig,

    #[serde(default)]
    pub host_id: String,

    /// Unique identifier for the fleet in which the super agent will join upon initialization.
    #[serde(default)]
    pub fleet_id: String,

    /// this is the only part of the config that can be changed with opamp.
    #[serde(flatten)]
    pub dynamic: SuperAgentDynamicConfig,

    /// opamp contains the OpAMP client configuration
    pub opamp: Option<OpAMPClientConfig>,

    // We could make this field available only when #[cfg(feature = "k8s")] but it would over-complicate
    // the struct definition and usage. Making it optional should work no matter what features are enabled.
    /// k8s is a map containing the kubernetes-specific settings
    #[serde(default)]
    pub k8s: Option<K8sConfig>,

    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
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

impl TryFrom<&str> for AgentTypeFQN {
    type Error = AgentTypeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let agent_type_fqn = AgentTypeFQN(value.to_string());

        if agent_type_fqn.namespace().is_empty() {
            return Err(AgentTypeError::InvalidAgentTypeNamespace);
        }
        if agent_type_fqn.name().is_empty() {
            return Err(AgentTypeError::InvalidAgentTypeName);
        }
        if agent_type_fqn.version().is_empty() {
            return Err(AgentTypeError::InvalidAgentTypeVersion);
        }

        Ok(agent_type_fqn)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SubAgentConfig {
    pub agent_type: AgentTypeFQN, // FQN of the agent type, ex: newrelic/nrdot:0.1.0
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct OpAMPClientConfig {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
/// K8sConfig represents the SuperAgent configuration for K8s environments
pub struct K8sConfig {
    /// cluster_name is an attribute used to identify all monitored data in a particular kubernetes cluster.
    pub cluster_name: String,
    /// namespace is the kubernetes namespace where all resources directly managed by the super agent will be created.
    pub namespace: String,

    /// CRDs is a list of crds that the SA should watch and be able to create/delete.
    #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
    #[serde(default = "default_group_version_kinds")]
    pub cr_type_meta: Vec<TypeMeta>,
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            cluster_name: String::new(),
            namespace: String::new(),
            cr_type_meta: default_group_version_kinds(),
        }
    }
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub fn helm_repository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1beta2".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub fn helm_release_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "helm.toolkit.fluxcd.io/v2beta2".to_string(),
        kind: "HelmRelease".to_string(),
    }
}

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
fn default_group_version_kinds() -> Vec<TypeMeta> {
    vec![helm_repository_type_meta(), helm_release_type_meta()]
}

impl AgentTypeFQN {
    pub(crate) fn get_capabilities(&self) -> Capabilities {
        //TODO: We should move this to EffectiveAgent
        default_capabilities()
    }
}

#[cfg(test)]
pub(crate) mod test {

    use std::path::PathBuf;

    use crate::logging::{
        file_logging::{FileLoggingConfig, LogFilePath},
        format::{LoggingFormat, TimestampFormat},
    };

    use super::*;

    const EXAMPLE_SUPERAGENT_CONFIG: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
log:
  format:
    target: true
    timestamp: "%Y"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const EXAMPLE_SUPERAGENT_CONFIG_NO_AGENTS: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
"#;

    const EXAMPLE_SUPERAGENT_CONFIG_EMPTY_AGENTS: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
agents: {}
"#;

    const EXAMPLE_SUBAGENTS_CONFIG: &str = r#"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const EXAMPLE_K8S_CONFIG: &str = r#"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
k8s:
  namespace: default
  cluster_name: some-cluster
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
  agent-1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const SUPERAGENT_CONFIG_UNKNOWN_AGENT_FIELDS: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  some-key: some-value
agents:
  agent-1:
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

    const SUPERAGENT_CONFIG_MISSING_K8S_FIELDS: &str = r#"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
k8s:
  cluster_name: some-cluster
  # the namespace is missing :(
"#;

    const SUPERAGENT_BAD_FILE_LOGGING_CONFIG: &str = r#"
log:
  file:
    path: /some/path
agents: {}
"#;

    const SUPERAGENT_FILE_LOGGING_CONFIG: &str = r#"
log:
  file:
    enable: true
    path: /some/path
agents: {}
"#;

    const SUPERAGENT_HOST_ID: &str = r#"
host_id: 123
agents: {}
"#;

    const SUPERAGENT_FLEET_ID: &str = r#"
fleet_id: 123
agents: {}
"#;

    impl From<HashMap<AgentID, SubAgentConfig>> for SuperAgentDynamicConfig {
        fn from(value: HashMap<AgentID, SubAgentConfig>) -> Self {
            Self { agents: value }
        }
    }

    #[test]
    fn agent_id_validator() {
        assert!(AgentID::try_from("ab".to_string()).is_ok());
        assert!(AgentID::try_from("a01b".to_string()).is_ok());
        assert!(AgentID::try_from("a-1-b".to_string()).is_ok());
        assert!(AgentID::try_from("a-1".to_string()).is_ok());
        assert!(AgentID::try_from("a".repeat(32)).is_ok());

        assert!(AgentID::try_from("A".to_string()).is_err());
        assert!(AgentID::try_from("1a".to_string()).is_err());
        assert!(AgentID::try_from("a".repeat(33)).is_err());
        assert!(AgentID::try_from("abc012-".to_string()).is_err());
        assert!(AgentID::try_from("-abc012".to_string()).is_err());
        assert!(AgentID::try_from("-".to_string()).is_err());
        assert!(AgentID::try_from("a.b".to_string()).is_err());
        assert!(AgentID::try_from("a*b".to_string()).is_err());
        assert!(AgentID::try_from("abc012/".to_string()).is_err());
        assert!(AgentID::try_from("/abc012".to_string()).is_err());
        assert!(AgentID::try_from("abc/012".to_string()).is_err());
        assert!(AgentID::try_from("aBc012".to_string()).is_err());
        assert!(AgentID::try_from("京bc012".to_string()).is_err());
        assert!(AgentID::try_from("s京123-12".to_string()).is_err());
        assert!(AgentID::try_from("super-agent-①".to_string()).is_err());
    }
    #[test]
    fn agent_type_fqn_validator() {
        assert!(AgentTypeFQN::try_from("ns/aa:1.1.3").is_ok());

        assert!(AgentTypeFQN::try_from("aa").is_err());
        assert!(AgentTypeFQN::try_from("aa:1.1.3").is_err());
        assert!(AgentTypeFQN::try_from("ns/-").is_err());
        assert!(AgentTypeFQN::try_from("ns/aa:").is_err());
        assert!(AgentTypeFQN::try_from("ns/:1.1.3").is_err());
        assert!(AgentTypeFQN::try_from("/:").is_err());
    }

    #[test]
    fn basic_parse() {
        assert!(serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_SUPERAGENT_CONFIG).is_ok());
        assert!(serde_yaml::from_str::<SuperAgentDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok());
        assert!(serde_yaml::from_str::<SuperAgentDynamicConfig>(EXAMPLE_K8S_CONFIG).is_ok());
        assert!(serde_yaml::from_str::<SuperAgentDynamicConfig>(
            EXAMPLE_SUPERAGENT_CONFIG_EMPTY_AGENTS
        )
        .is_ok());
        assert!(
            serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_SUPERAGENT_CONFIG_NO_AGENTS).is_err()
        );
        assert!(serde_yaml::from_str::<SuperAgentDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok())
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
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("AgentID must contain 32 characters at most, contain alphanumeric characters or dashes only, start with alphabetic, and end with alphanumeric"))
    }

    #[test]
    fn parse_with_reserved_agent_id() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_RESERVED_AGENT_ID);
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("AgentID 'super-agent' is reserved at line"))
    }

    #[test]
    fn parse_with_missing_k8s_fields() {
        let actual = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_CONFIG_MISSING_K8S_FIELDS);
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("k8s: missing field"));
    }

    #[test]
    fn test_agent_type_fqn() {
        let fqn: AgentTypeFQN = "newrelic/nrdot:0.1.0".try_into().unwrap();
        assert_eq!(fqn.namespace(), "newrelic");
        assert_eq!(fqn.name(), "nrdot");
        assert_eq!(fqn.version(), "0.1.0");
    }

    #[test]
    fn bad_agent_type_fqn_no_version() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "newrelic/nrdot".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_no_name() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "newrelic/:0.1.0".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "/nrdot:0.1.0".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace_no_version() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "/nrdot".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_no_namespace_no_name() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "/:0.1.0".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_namespace_separator() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "/".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_empty_string() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_only_version_separator() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = ":".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn bad_agent_type_fqn_only_word() {
        let fqn: Result<AgentTypeFQN, AgentTypeError> = "only_namespace".try_into();
        assert!(fqn.is_err());
    }

    #[test]
    fn test_logging_config() {
        let default_config =
            serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_SUPERAGENT_CONFIG_EMPTY_AGENTS);
        assert!(default_config.is_ok());
        let custom_config = serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_SUPERAGENT_CONFIG);
        assert!(custom_config.is_ok());
        assert_eq!(default_config.unwrap().log, LoggingConfig::default());
        assert_eq!(
            custom_config.unwrap().log,
            LoggingConfig {
                format: LoggingFormat {
                    target: true,
                    timestamp: TimestampFormat("%Y".to_string())
                },
                ..Default::default()
            }
        );
    }

    #[test]
    fn log_path_but_not_enabled_should_error() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_BAD_FILE_LOGGING_CONFIG);
        assert!(config.is_err());
        assert_eq!(
            config.unwrap_err().to_string(),
            "log.file: missing field `enable` at line 4 column 5"
        );
    }

    #[test]
    fn good_file_logging_config() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_FILE_LOGGING_CONFIG);
        assert!(config.is_ok());
        assert_eq!(
            config.unwrap().log.file,
            FileLoggingConfig {
                enable: true,
                path: LogFilePath::try_from(PathBuf::from("/some/path")).unwrap(),
            }
        );
    }

    #[test]
    fn host_id_config() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_HOST_ID).unwrap();
        assert_eq!(config.host_id, "123");
    }

    #[test]
    fn fleet_id_config() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_FLEET_ID).unwrap();
        assert_eq!(config.fleet_id, "123");
    }
}
