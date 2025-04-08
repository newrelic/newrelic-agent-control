use super::agent_id::AgentID;
use super::http_server::config::ServerConfig;
use super::uptime_report::UptimeReportConfig;
use crate::http::config::ProxyConfig;
use crate::instrumentation::config::logs::config::LoggingConfig;
use crate::opamp::auth::config::AuthConfig;
use crate::opamp::remote_config::validators::signature::validator::SignatureValidatorConfig;
use crate::opamp::remote_config::RemoteConfigError;
use crate::values::yaml_config::YAMLConfig;
use crate::{
    agent_type::agent_type_id::AgentTypeID, instrumentation::config::InstrumentationConfig,
};
use http::HeaderMap;

use kube::api::TypeMeta;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use url::Url;

/// AgentControlConfig represents the configuration for the agent control.
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct AgentControlConfig {
    #[serde(default)]
    pub log: LoggingConfig,

    #[serde(default)]
    pub host_id: String,

    /// this is the only part of the config that can be changed with opamp.
    #[serde(flatten)]
    pub dynamic: AgentControlDynamicConfig,

    /// fleet_control contains the OpAMP client configuration
    pub fleet_control: Option<OpAMPClientConfig>,

    // We could make this field available only when #[cfg(feature = "k8s")] but it would over-complicate
    // the struct definition and usage. Making it optional should work no matter what features are enabled.
    /// k8s is a map containing the kubernetes-specific settings
    #[serde(default)]
    pub k8s: Option<K8sConfig>,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub proxy: ProxyConfig,

    #[serde(default)]
    pub self_instrumentation: InstrumentationConfig,

    #[serde(default)]
    pub uptime_report: Option<UptimeReportConfig>,
}

#[derive(Error, Debug)]
pub enum AgentControlConfigError {
    #[error("deleting agent control config: `{0}`")]
    Delete(String),
    #[error("loading agent control config: `{0}`")]
    Load(String),
    #[error("storing agent control config: `{0}`")]
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

/// AgentControlDynamicConfig represents the dynamic part of the agentControl config.
/// The dynamic configuration can be changed remotely.
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct AgentControlDynamicConfig {
    pub agents: SubAgentsMap,
}

pub type SubAgentsMap = HashMap<AgentID, SubAgentConfig>;

impl TryFrom<&str> for AgentControlDynamicConfig {
    type Error = AgentControlConfigError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(serde_yaml::from_str(value)?)
    }
}

impl TryFrom<YAMLConfig> for AgentControlConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

impl TryFrom<&AgentControlDynamicConfig> for YAMLConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: &AgentControlDynamicConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

impl TryFrom<YAMLConfig> for AgentControlDynamicConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct SubAgentConfig {
    #[serde(serialize_with = "AgentTypeID::serialize_fqn")]
    #[serde(deserialize_with = "AgentTypeID::deserialize_fqn")]
    pub agent_type: AgentTypeID,
}

#[derive(Debug, PartialEq, Serialize, Clone)]
pub struct OpAMPClientConfig {
    pub endpoint: Url,
    #[serde(with = "http_serde::header_map")]
    pub headers: HeaderMap,
    pub auth_config: Option<AuthConfig>,
    /// Unique identifier for the fleet in which the super agent will join upon initialization.
    pub fleet_id: String,
    /// Contains the signature_validation configuration
    #[serde(default)]
    pub signature_validation: SignatureValidatorConfig,
}

impl<'de> Deserialize<'de> for OpAMPClientConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // intermediate serialization type to validate `default` and `required` fields
        #[derive(Debug, Deserialize)]
        struct IntermediateOpAMPClientConfig {
            #[serde(default)]
            fleet_id: String,
            endpoint: Url,
            #[serde(default, with = "http_serde::header_map")]
            headers: HeaderMap,
            auth_config: Option<AuthConfig>,
            #[serde(default)]
            pub signature_validation: SignatureValidatorConfig,
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
            fleet_id: intermediate_spec.fleet_id,
            endpoint: intermediate_spec.endpoint,
            headers: censored_headers,
            auth_config: intermediate_spec.auth_config,
            signature_validation: intermediate_spec.signature_validation,
        })
    }
}

/// K8sConfig represents the AgentControl configuration for K8s environments
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct K8sConfig {
    /// cluster_name is an attribute used to identify all monitored data in a particular kubernetes cluster.
    pub cluster_name: String,
    /// namespace is the kubernetes namespace where all resources directly managed by the agent control will be created.
    pub namespace: String,
    /// chart_version is the version of the chart used to deploy agent control
    #[serde(default)]
    pub chart_version: String,

    /// CRDs is a list of crds that the SA should watch and be able to create/delete.
    #[serde(default = "default_group_version_kinds")]
    pub cr_type_meta: Vec<TypeMeta>,
}

impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            cluster_name: Default::default(),
            namespace: Default::default(),
            chart_version: Default::default(),
            cr_type_meta: default_group_version_kinds(),
        }
    }
}

pub fn helmrelease_v2_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "helm.toolkit.fluxcd.io/v2".to_string(),
        kind: "HelmRelease".to_string(),
    }
}

pub fn instrumentation_v1beta1_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "newrelic.com/v1beta1".to_string(),
        kind: "Instrumentation".to_string(),
    }
}

pub fn default_group_version_kinds() -> Vec<TypeMeta> {
    // In flux health check we are currently supporting just a single helm_release_type_meta
    // Each time we support a new version we should decide if and how to support retrieving its health
    // A dynamic object reflector will be created for each of these types, since the GC lists them.
    vec![
        // Agent Operator CRD
        instrumentation_v1beta1_type_meta(),
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
        helmrelease_v2_type_meta(),
    ]
}

#[cfg(test)]
pub(crate) mod tests {

    use std::path::PathBuf;

    use crate::instrumentation::config::logs::{
        file_logging::{FileLoggingConfig, LogFilePath},
        format::{LoggingFormat, TimestampFormat},
    };

    use super::*;

    impl Default for OpAMPClientConfig {
        fn default() -> Self {
            OpAMPClientConfig {
                fleet_id: String::default(),
                endpoint: "http://localhost".try_into().unwrap(),
                headers: HeaderMap::default(),
                auth_config: None,
                signature_validation: Default::default(),
            }
        }
    }

    const EXAMPLE_AGENTCONTROL_CONFIG: &str = r#"
fleet_control:
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

    const EXAMPLE_AGENTCONTROL_CONFIG_NO_AGENTS: &str = r#"
fleet_control:
  endpoint: http://localhost:8080/some/path
  headers:
    some-key: some-value
"#;

    const EXAMPLE_AGENTCONTROL_CONFIG_EMPTY_AGENTS: &str = r#"
fleet_control:
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

    const AGENTCONTROL_CONFIG_WRONG_AGENT_ID: &str = r#"
agents:
  agent/1:
    agent_type: namespace/agent_type:0.0.1
"#;

    const AGENTCONTROL_CONFIG_RESERVED_AGENT_ID: &str = r#"
agents:
  agent-control:
    agent_type: namespace/agent_type:0.0.1
"#;

    const AGENTCONTROL_CONFIG_MISSING_K8S_FIELDS: &str = r#"
agents:
  agent-1:
    agent_type: namespace/agent_type:0.0.1
k8s:
  cluster_name: some-cluster
  # the namespace is missing :(
"#;

    const AGENTCONTROL_BAD_FILE_LOGGING_CONFIG: &str = r#"
log:
  file:
    path: /some/path
agents: {}
"#;

    const AGENTCONTROL_FILE_LOGGING_CONFIG: &str = r#"
log:
  file:
    enabled: true
    path: /some/path
agents: {}
"#;

    const AGENTCONTROL_HOST_ID: &str = r#"
host_id: 123
agents: {}
"#;

    const AGENTCONTROL_FLEET_ID: &str = r#"
fleet_control:
  endpoint: http://localhost:8080/some/path
  fleet_id: 123
agents: {}
"#;

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

    const AGENTCONTROL_PROXY: &str = r#"
proxy:
  url: http://localhost:8080
agents: {}
"#;

    impl From<HashMap<AgentID, SubAgentConfig>> for AgentControlDynamicConfig {
        fn from(value: HashMap<AgentID, SubAgentConfig>) -> Self {
            Self { agents: value }
        }
    }

    #[test]
    fn basic_parse() {
        assert!(serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_AGENTCONTROL_CONFIG).is_ok());
        assert!(
            serde_yaml::from_str::<AgentControlDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok()
        );
        assert!(serde_yaml::from_str::<AgentControlDynamicConfig>(EXAMPLE_K8S_CONFIG).is_ok());
        assert!(serde_yaml::from_str::<AgentControlDynamicConfig>(
            EXAMPLE_AGENTCONTROL_CONFIG_EMPTY_AGENTS
        )
        .is_ok());
        assert!(
            serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_AGENTCONTROL_CONFIG_NO_AGENTS)
                .is_err()
        );
        assert!(
            serde_yaml::from_str::<AgentControlDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok()
        );
    }

    #[test]
    fn parse_with_wrong_agent_id() {
        let actual = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_CONFIG_WRONG_AGENT_ID);
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("AgentID must contain 32 characters at most, contain lowercase alphanumeric characters or dashes only, start with alphabetic, and end with alphanumeric"))
    }

    #[test]
    fn parse_with_reserved_agent_id() {
        let actual =
            serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_CONFIG_RESERVED_AGENT_ID);
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("AgentID 'agent-control' is reserved at line"))
    }

    #[test]
    fn parse_with_missing_k8s_fields() {
        let actual =
            serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_CONFIG_MISSING_K8S_FIELDS);
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("k8s: missing field"));
    }

    #[test]
    fn test_logging_config() {
        let default_config =
            serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_AGENTCONTROL_CONFIG_EMPTY_AGENTS);
        assert!(default_config.is_ok());
        let custom_config = serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_AGENTCONTROL_CONFIG);
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
        let config =
            serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_BAD_FILE_LOGGING_CONFIG);
        assert!(config.is_err());
        assert_eq!(
            config.unwrap_err().to_string(),
            "log.file: missing field `enabled` at line 4 column 5"
        );
    }

    #[test]
    fn good_file_logging_config() {
        let config = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_FILE_LOGGING_CONFIG);
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
        let config = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_HOST_ID).unwrap();
        assert_eq!(config.host_id, "123");
    }

    #[test]
    fn fleet_id_config() {
        let config = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_FLEET_ID).unwrap();
        assert_eq!(config.fleet_control.unwrap().fleet_id, "123");
    }

    #[test]
    fn k8s_cr_config() {
        let config =
            serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_K8S_EXTRA_CR_CONFIG).unwrap();
        let custom_type_meta = TypeMeta {
            api_version: "custom.io/v1".to_string(),
            kind: "CustomKind".to_string(),
        };
        assert_eq!(config.k8s.unwrap().cr_type_meta, vec![custom_type_meta]);

        let config = serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_K8S_CONFIG).unwrap();
        assert_eq!(
            config.k8s.unwrap().cr_type_meta,
            default_group_version_kinds()
        );
    }

    #[test]
    fn test_proxy_config() {
        let config = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_PROXY).unwrap();
        assert_eq!(
            config.proxy.url_as_string(),
            "http://localhost:8080/".to_string()
        )
    }
}
