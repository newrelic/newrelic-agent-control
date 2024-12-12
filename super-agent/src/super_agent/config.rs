use super::http_server::config::ServerConfig;
use crate::http::proxy::ProxyConfig;
use crate::logging::config::LoggingConfig;
use crate::opamp::auth::config::AuthConfig;
use crate::opamp::remote_config::RemoteConfigError;
use crate::super_agent::defaults::{default_capabilities, SUPER_AGENT_ID};
use crate::values::yaml_config::YAMLConfig;
use http::HeaderMap;
#[cfg(feature = "k8s")]
use kube::api::TypeMeta;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Deref;
use std::path::Path;
use std::{collections::HashMap, fmt::Display};
use thiserror::Error;
use url::Url;

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
    #[error("deleting super agent config: `{0}`")]
    Delete(String),
    #[error("loading super agent config: `{0}`")]
    Load(String),
    #[error("storing super agent config: `{0}`")]
    Store(String),
    #[error("building source to parse environment variables: `{0}`")]
    ConfigError(#[from] config::ConfigError),
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

impl TryFrom<YAMLConfig> for SuperAgentConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

impl TryFrom<&SuperAgentDynamicConfig> for YAMLConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: &SuperAgentDynamicConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

impl TryFrom<YAMLConfig> for SuperAgentDynamicConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
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

    #[serde(default)]
    pub proxy: ProxyConfig,
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
pub struct SubAgentConfig {
    pub agent_type: AgentTypeFQN, // FQN of the agent type, ex: newrelic/nrdot:0.1.0
}

#[derive(Debug, PartialEq, Serialize, Clone)]
pub struct OpAMPClientConfig {
    pub endpoint: Url,
    #[serde(with = "http_serde::header_map")]
    pub headers: HeaderMap,
    pub auth_config: Option<AuthConfig>,
}

impl<'de> Deserialize<'de> for OpAMPClientConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // intermediate serialization type to validate `default` and `required` fields
        #[derive(Debug, Deserialize)]
        struct IntermediateOpAMPClientConfig {
            pub endpoint: Url,
            #[serde(default, with = "http_serde::header_map")]
            pub headers: HeaderMap,
            pub auth_config: Option<AuthConfig>,
        }

        let mut intermediate_spec = IntermediateOpAMPClientConfig::deserialize(deserializer)?;

        let censored_headers = intermediate_spec
            .headers
            .iter_mut()
            .map(|(header_name, header_value)| {
                let _name = header_name.to_string();
                // TODO: Find a way to properly censor these values.
                if header_name == "api-key" {
                    header_value.set_sensitive(true);
                }
                (header_name.to_owned(), header_value.to_owned())
            })
            .collect::<HeaderMap>();

        Ok(OpAMPClientConfig {
            endpoint: intermediate_spec.endpoint,
            headers: censored_headers,
            auth_config: intermediate_spec.auth_config,
        })
    }
}

/// K8sConfig represents the SuperAgent configuration for K8s environments
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct K8sConfig {
    /// cluster_name is an attribute used to identify all monitored data in a particular kubernetes cluster.
    pub cluster_name: String,
    /// namespace is the kubernetes namespace where all resources directly managed by the super agent will be created.
    pub namespace: String,

    /// CRDs is a list of crds that the SA should watch and be able to create/delete.
    #[cfg(feature = "k8s")]
    #[serde(default = "default_group_version_kinds")]
    pub cr_type_meta: Vec<TypeMeta>,
}

#[cfg(feature = "k8s")]
impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            cluster_name: String::new(),
            namespace: String::new(),
            cr_type_meta: default_group_version_kinds(),
        }
    }
}

#[cfg(feature = "k8s")]
pub fn helm_release_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "helm.toolkit.fluxcd.io/v2".to_string(),
        kind: "HelmRelease".to_string(),
    }
}

#[cfg(feature = "k8s")]
pub fn default_group_version_kinds() -> Vec<TypeMeta> {
    // In flux health check we are currently supporting just a single helm_release_type_meta
    // Each time we support a new version we should decide if and how to support retrieving its health
    // A dynamic object reflector will be created for each of these types, since the GC lists them.
    vec![
        // Agent Operator CRD
        TypeMeta {
            api_version: "newrelic.com/v1alpha2".to_string(),
            kind: "Instrumentation".to_string(),
        },
        // This allows Secrets created as dynamic objects to be cleaned up by the GC
        // This should not be needed anymore whenever the GC detection logic doesn't rely on this list.
        TypeMeta {
            api_version: "v1".to_string(),
            kind: "Secret".to_string(),
        },
        TypeMeta {
            api_version: "source.toolkit.fluxcd.io/v1".to_string(),
            kind: "HelmRepository".to_string(),
        },
        helm_release_type_meta(),
    ]
}

impl AgentTypeFQN {
    pub(crate) fn get_capabilities(&self) -> Capabilities {
        //TODO: We should move this to EffectiveAgent
        default_capabilities()
    }
}

#[cfg(test)]
pub(crate) mod tests {

    use std::path::PathBuf;

    use crate::logging::{
        file_logging::{FileLoggingConfig, LogFilePath},
        format::{LoggingFormat, TimestampFormat},
    };

    use super::*;

    impl Default for OpAMPClientConfig {
        fn default() -> Self {
            OpAMPClientConfig {
                endpoint: "http://localhost".try_into().unwrap(),
                headers: HeaderMap::default(),
                auth_config: None,
            }
        }
    }

    const EXAMPLE_SUPERAGENT_CONFIG: &str = r#"
opamp:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
  auth_config:
    token_url: "http://fake.com/oauth2/v1/token"
    client_id: "fake"
    provider: "local"
    private_key_path: "path/to/key"
log:
  format:
    target: true
    timestamp: "%Y"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
proxy:
  url: http://localhost:8080
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
    enabled: true
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

    #[cfg(feature = "k8s")]
    const EXAMPLE_K8S_EXTRA_CR_CONFIG: &str = r#"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
k8s:
  namespace: default
  cluster_name: some-cluster
  cr_type_meta:
    - apiVersion: "custom.io/v1"
      kind: "CustomKind"
"#;

    const SUPERAGENT_PROXY: &str = r#"
proxy:
  url: http://localhost:8080
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
        assert!(serde_yaml::from_str::<SuperAgentDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok());
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
                enabled: true,
                path: Some(LogFilePath::try_from(PathBuf::from("/some/path")).unwrap()),
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

    #[cfg(feature = "k8s")]
    #[test]
    fn k8s_cr_config() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_K8S_EXTRA_CR_CONFIG).unwrap();
        let custom_type_meta = TypeMeta {
            api_version: "custom.io/v1".to_string(),
            kind: "CustomKind".to_string(),
        };
        assert_eq!(config.k8s.unwrap().cr_type_meta, vec![custom_type_meta]);

        let config = serde_yaml::from_str::<SuperAgentConfig>(EXAMPLE_K8S_CONFIG).unwrap();
        assert_eq!(
            config.k8s.unwrap().cr_type_meta,
            default_group_version_kinds()
        );
    }

    #[test]
    fn test_proxy_config() {
        let config = serde_yaml::from_str::<SuperAgentConfig>(SUPERAGENT_PROXY).unwrap();
        assert_eq!(
            config.proxy.url_as_string(),
            "http://localhost:8080/".to_string()
        )
    }
}
