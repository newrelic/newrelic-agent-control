use super::agent_id::AgentID;
use super::http_server::config::ServerConfig;
use super::uptime_report::UptimeReportConfig;
use crate::agent_control::health_checker::AgentControlHealthCheckerConfig;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::http::config::ProxyConfig;
use crate::instrumentation::config::logs::config::LoggingConfig;
use crate::opamp::auth::config::AuthConfig;
use crate::opamp::client_builder::PollInterval;
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::validators::signature::validator::SignatureValidatorConfig;
use crate::secrets_provider::SecretsProvidersConfig;
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
use wrapper_with_default::WrapperWithDefault;

/// AgentControlConfig represents the configuration for the agent control.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
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

    /// kubernetes-specific settings
    #[serde(default)]
    pub k8s: Option<K8sConfig>,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub proxy: ProxyConfig,

    #[serde(default)]
    pub self_instrumentation: InstrumentationConfig,

    #[serde(default)]
    pub uptime_report: UptimeReportConfig,

    #[serde(default)]
    pub health_check: AgentControlHealthCheckerConfig,

    /// A "key-value store" intended to modify agent type definitions, loaded at start time.
    #[serde(default)]
    pub agent_type_var_constraints: VariableConstraints,

    /// configuration for every secrets provider that the current AgentControl instance should be able to access
    #[serde(default)]
    pub secrets_providers: Option<SecretsProvidersConfig>,

    /// Contains the configuration related to host agent packages
    #[serde(default)]
    pub agent_packages: PackagesConfig,
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
pub struct PackagesConfig {
    /// Indicates whether package signature verification is enabled or not
    pub signature_verification_enabled: SignatureVerificationEnabled,
}

const DEFAULT_SIGNATURE_VERIFICATION_ENABLED: bool = true;
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_SIGNATURE_VERIFICATION_ENABLED)]
pub struct SignatureVerificationEnabled(bool);

impl TryFrom<YAMLConfig> for AgentControlConfig {
    type Error = serde_yaml::Error;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(serde_yaml::to_value(value)?)
    }
}

#[derive(Error, Debug)]
#[error("{0}")]
pub struct AgentControlConfigError(pub String);

pub type SubAgentsMap = HashMap<AgentID, SubAgentConfig>;

/// Key for the agents section in the configuration.
/// There is an special merge behavior for this key when processing remote configs.
const AGENTS_KEY: &str = "agents";

/// AgentControlDynamicConfig represents the dynamic part of the agentControl config.
/// The dynamic configuration can be changed remotely.
#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct AgentControlDynamicConfig {
    pub agents: SubAgentsMap,
    /// chart_version represent the AC version that needs to be executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_version: Option<String>,
    /// cd_chart_version represent the agent control cd chart version that needs to be executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cd_chart_version: Option<String>,
}

/// This implementation reads all configuration entries whose keys start with
/// [AGENT_CONFIG_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_PREFIX) and produces
/// a single dynamic config:
/// - Merges top-level YAML keys across configs, erroring on duplicate non-`agents` keys.
/// - Applies special merge semantics for the `agents` key: combines agent maps and errors
///   on duplicate agent IDs.
/// - Preserves optional version fields like `chart_version` and `cd_chart_version`, failing
///   if multiple different configs define the same version key.
///
/// # Example
///
/// ```json
/// // Input (OpAMP config map):
/// {
///   "agentConfig-config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}}",
///   "agentConfig-config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}}"
/// }
///
/// // Resulting dynamic config:
/// {
///   "agents": {
///     "agent-a": {"agent_type": "foo/bar:0.0.1"},
///     "agent-b": {"agent_type": "foo/bar:0.0.1"}
///   }
/// }
/// ```
impl TryFrom<&OpampRemoteConfig> for AgentControlDynamicConfig {
    type Error = AgentControlConfigError;
    fn try_from(value: &OpampRemoteConfig) -> Result<Self, Self::Error> {
        let mut merged_agents = SubAgentsMap::new();
        let mut remaining_config = YAMLConfig::try_from(&AgentControlDynamicConfig::default())
            .map_err(|err| {
                AgentControlConfigError(format!("initializing default config: {}", err))
            })?;

        for (config_name, config_content) in value.agent_configs_iter() {
            let mut yaml_configuration =
                YAMLConfig::try_from(config_content.as_str()).map_err(|err| {
                    AgentControlConfigError(format!("invalid config '{}': {}", config_name, err))
                })?;

            if let Some(agents_value) = yaml_configuration.remove_key(AGENTS_KEY) {
                merged_agents = try_append_agents(merged_agents, agents_value).map_err(|err| {
                    AgentControlConfigError(format!(
                        "appending agents from config '{}': {}",
                        config_name, err
                    ))
                })?;
            }

            remaining_config = YAMLConfig::try_append(remaining_config.clone(), yaml_configuration)
                .map_err(|err| AgentControlConfigError(format!("appending config: {err}")))?;
        }

        let remaining_config_appended = AgentControlDynamicConfig::try_from(remaining_config)
            .map_err(|err| AgentControlConfigError(format!("encoding config: {}", err)))?;

        Ok(AgentControlDynamicConfig {
            agents: merged_agents,
            ..remaining_config_appended
        })
    }
}

/// Tries to append agents from a YAML value into the agents map, erroring on duplicates.
fn try_append_agents(
    merged_agents: SubAgentsMap,
    agents_value: serde_yaml::Value,
) -> Result<SubAgentsMap, AgentControlConfigError> {
    let sub_agents_map: SubAgentsMap = serde_yaml::from_value(agents_value)
        .map_err(|err| AgentControlConfigError(format!("invalid agents: {}", err)))?;

    let mut merged_agents = merged_agents;

    for (agent_id, agent_config) in sub_agents_map {
        if merged_agents
            .insert(agent_id.clone(), agent_config)
            .is_some()
        {
            return Err(AgentControlConfigError(format!(
                "duplicated agent: {}",
                agent_id
            )));
        }
    }

    Ok(merged_agents)
}

impl TryFrom<YAMLConfig> for AgentControlDynamicConfig {
    type Error = AgentControlConfigError;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(
            serde_yaml::to_value(value)
                .map_err(|e| AgentControlConfigError(format!("deserializing: {e}")))?,
        )
        .map_err(|e| AgentControlConfigError(format!("serializing: {e}")))
    }
}

/// Return elements of the first map not existing in the second map.
pub fn sub_agents_difference<'a>(
    old_sub_agents: &'a SubAgentsMap,
    new_sub_agents: &'a SubAgentsMap,
) -> impl Iterator<Item = (&'a AgentID, &'a SubAgentConfig)> {
    old_sub_agents
        .iter()
        .filter(|(agent_id, _)| !new_sub_agents.contains_key(agent_id))
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct SubAgentConfig {
    #[serde(serialize_with = "AgentTypeID::serialize_fqn")]
    #[serde(deserialize_with = "AgentTypeID::deserialize_fqn")]
    pub agent_type: AgentTypeID,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
pub struct AuthSecret {
    /// The name of the Kubernetes Secret resource.
    /// Defaults to "newrelic-agent-control-secret" if not specified.
    #[serde(default)]
    pub secret_name: String,

    /// The key within the Secret data map where the value is stored.
    /// Defaults to "api-key" if not specified.
    #[serde(default)]
    pub secret_key_name: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct OpAMPClientConfig {
    /// OpAMP server endpoint.
    pub endpoint: Url,
    /// Poll interval for the OpAMP client.
    pub poll_interval: PollInterval,
    /// Headers to be sent with the OpAMP requests.
    pub headers: HeaderMap,
    /// Authentication configuration for the OpAMP communications.
    pub auth_config: Option<AuthConfig>,
    /// Unique identifier for the fleet in which the super agent will join upon initialization.
    pub fleet_id: String,
    /// Contains the signature_validation configuration
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
            endpoint: Url,
            #[serde(default)]
            poll_interval: PollInterval,
            #[serde(default, with = "http_serde::header_map")]
            headers: HeaderMap,
            #[serde(default)]
            auth_config: Option<AuthConfig>,
            #[serde(default)]
            fleet_id: String,
            #[serde(default)]
            signature_validation: SignatureValidatorConfig,
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
            poll_interval: intermediate_spec.poll_interval,
            headers: censored_headers,
            auth_config: intermediate_spec.auth_config,
            fleet_id: intermediate_spec.fleet_id,
            signature_validation: intermediate_spec.signature_validation,
        })
    }
}

/// K8sConfig represents the AgentControl configuration for K8s environments
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct K8sConfig {
    /// cluster_name is an attribute used to identify all monitored data in a particular kubernetes cluster. Required
    pub cluster_name: String,
    /// namespace where all resources directly managed by the agent control will be created.
    pub namespace: String,
    /// namespace where all resources managed by flux will be created.
    pub namespace_agents: String,
    /// current_chart_version is the version of the chart used to deploy agent control
    /// This value is passed to the agent control via Environment Variable to avoid race conditions.
    /// If set via config, after a failed upgrade we could have the "old" pod loading the new config
    /// and reading the new chart version, while the image is still the old one.
    #[serde(default)]
    pub current_chart_version: String,
    /// CRDs is a list of crds that AC should watch and be able to create/delete.
    #[serde(default = "default_group_version_kinds")]
    pub cr_type_meta: Vec<TypeMeta>,
    /// ac_remote_update enables or disables remote update for agent-control-deployment chart
    #[serde(default)]
    pub ac_remote_update: bool,
    /// agent_control_deployment release name
    #[serde(default)]
    pub ac_release_name: String,
    /// cd_remote_update enables or disables remote update for the agent-control-cd chart
    #[serde(default)]
    pub cd_remote_update: bool,
    /// agent_control_cd release name
    #[serde(default)]
    pub cd_release_name: String,
    /// Specifies the key name within the Kubernetes Secret
    /// used to retrieve the required secret for credentials.
    #[serde(default)]
    pub auth_secret: AuthSecret,
}

pub fn helmrelease_v2_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "helm.toolkit.fluxcd.io/v2".to_string(),
        kind: "HelmRelease".to_string(),
    }
}

pub fn instrumentation_v1beta3_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "newrelic.com/v1beta3".to_string(),
        kind: "Instrumentation".to_string(),
    }
}

pub fn helmrepository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

pub fn helmchart_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmChart".to_string(),
    }
}

pub fn statefulset_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "apps/v1".to_string(),
        kind: "StatefulSet".to_string(),
    }
}

pub fn daemonset_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "apps/v1".to_string(),
        kind: "DaemonSet".to_string(),
    }
}

pub fn deployment_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "apps/v1".to_string(),
        kind: "Deployment".to_string(),
    }
}

pub fn default_group_version_kinds() -> Vec<TypeMeta> {
    // In flux health check we are currently supporting just a single helm_release_type_meta
    // Each time we support a new version we should decide if and how to support retrieving its health
    // A dynamic object reflector will be created for each of these types, since the GC lists them.
    vec![
        // Agent Operator CRD
        instrumentation_v1beta3_type_meta(),
        // This allows Secrets created as dynamic objects to be cleaned up by the GC
        // This should not be needed anymore whenever the GC detection logic doesn't rely on this list.
        TypeMeta {
            api_version: "v1".to_string(),
            kind: "Secret".to_string(),
        },
        helmrepository_type_meta(),
        helmrelease_v2_type_meta(),
    ]
}

impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            cluster_name: Default::default(),
            namespace: Default::default(),
            namespace_agents: Default::default(),
            current_chart_version: Default::default(),
            cr_type_meta: default_group_version_kinds(),
            ac_remote_update: Default::default(),
            ac_release_name: Default::default(),
            cd_remote_update: Default::default(),
            cd_release_name: Default::default(),
            auth_secret: AuthSecret {
                secret_name: Default::default(),
                secret_key_name: Default::default(),
            },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::{AGENT_CONFIG_PREFIX, ConfigurationMap, OpampRemoteConfig};
    use crate::{
        instrumentation::config::logs::{
            file_logging::{FileLoggingConfig, LogFilePath},
            format::{LoggingFormat, TimestampFormat},
        },
        sub_agent::identity::AgentIdentity,
    };
    use assert_matches::assert_matches;
    use rstest::rstest;
    use std::path::PathBuf;

    impl TryFrom<&str> for AgentControlDynamicConfig {
        type Error = AgentControlConfigError;
        fn try_from(value: &str) -> Result<Self, Self::Error> {
            serde_yaml::from_str(value)
                .map_err(|e| AgentControlConfigError(format!("serializing: {e}")))
        }
    }

    impl Default for OpAMPClientConfig {
        fn default() -> Self {
            OpAMPClientConfig {
                fleet_id: String::default(),
                endpoint: "http://localhost".try_into().unwrap(),
                poll_interval: PollInterval::default(),
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
  namespace_agents: default
  ac_remote_update: true
  cd_remote_update: true
  auth_secret:
    secret_name: "secret-name"
    secret_key_name: "secret-key"
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

    const AGENTCONTROL_PROXY: &str = r#"
proxy:
  url: http://localhost:8080
agents: {}
"#;

    #[test]
    fn basic_parse() {
        assert!(serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_AGENTCONTROL_CONFIG).is_ok());
        assert!(
            serde_yaml::from_str::<AgentControlDynamicConfig>(EXAMPLE_SUBAGENTS_CONFIG).is_ok()
        );
        let k8s_config = serde_yaml::from_str::<AgentControlConfig>(EXAMPLE_K8S_CONFIG);
        assert!(k8s_config.is_ok());
        let k8s = k8s_config.unwrap().k8s.unwrap();
        assert_eq!(k8s.auth_secret.secret_name, "secret-name");
        assert_eq!(k8s.auth_secret.secret_key_name, "secret-key");
        assert!(
            serde_yaml::from_str::<AgentControlDynamicConfig>(
                EXAMPLE_AGENTCONTROL_CONFIG_EMPTY_AGENTS
            )
            .is_ok()
        );
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
        assert!(
            actual
                .unwrap_err()
                .to_string()
                .contains("AgentID 'agent-control' is reserved at line")
        )
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
                    timestamp: TimestampFormat("%Y".to_string()),
                    ansi_colors: false,
                    formatter: Default::default(),
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
    fn test_ac_k8s_required_config_only() {
        let config_input = r#"
agents: {}
k8s:
  namespace: some-namespace
  namespace_agents: some-namespace-agents
  cluster_name: some-cluster
"#;

        let config = serde_yaml::from_str::<AgentControlConfig>(config_input).unwrap();

        let k8s = config.k8s.unwrap();

        assert_eq!(k8s.namespace_agents, "some-namespace-agents");
        assert_eq!(k8s.cluster_name, "some-cluster");
        assert_eq!(k8s.namespace, "some-namespace");
    }

    #[test]
    fn test_ac_k8s_fail_when_missing_required_field() {
        let missing_namespace = r#"
agents: {}
k8s:
  # missing namespace
  cluster_name: some-cluster
"#;
        assert!(
            serde_yaml::from_str::<AgentControlConfig>(missing_namespace)
                .unwrap_err()
                .to_string()
                .contains("k8s: missing field `namespace`")
        );

        let missing_cluster_name = r#"
agents: {}
k8s:
  namespace: some
  # missing cluster_name
"#;
        assert!(
            serde_yaml::from_str::<AgentControlConfig>(missing_cluster_name)
                .unwrap_err()
                .to_string()
                .contains("k8s: missing field `cluster_name`")
        );
    }

    #[test]
    fn k8s_all_config() {
        let config_input = r#"
agents: {}
k8s:
  namespace: some-namespace
  namespace_agents: some-namespace-agents
  cluster_name: some-cluster
  cr_type_meta:
    - apiVersion: "custom.io/v1"
      kind: "CustomKind"
"#;

        let config = serde_yaml::from_str::<AgentControlConfig>(config_input).unwrap();

        let custom_type_meta = TypeMeta {
            api_version: "custom.io/v1".to_string(),
            kind: "CustomKind".to_string(),
        };

        let k8s = config.k8s.unwrap();

        assert_eq!(k8s.cr_type_meta, vec![custom_type_meta]);
        assert_eq!(k8s.namespace, "some-namespace");
        assert_eq!(k8s.cluster_name, "some-cluster");
    }

    #[test]
    fn test_proxy_config() {
        let config = serde_yaml::from_str::<AgentControlConfig>(AGENTCONTROL_PROXY).unwrap();
        assert_eq!(
            config.proxy.url_as_string(),
            "http://localhost:8080/".to_string()
        )
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Tests for sub_agents_difference function
    ////////////////////////////////////////////////////////////////////////////////////

    #[test]
    fn test_sub_agent_removal_diff_no_removal() {
        let old_sub_agents = helper_get_agent_list();

        let new_sub_agents = old_sub_agents.clone();

        let diff: Vec<_> = sub_agents_difference(&old_sub_agents, &new_sub_agents).collect();

        assert!(diff.is_empty());
    }

    #[test]
    fn test_sub_agent_removal_diff_with_removal() {
        let old_sub_agents = helper_get_agent_list();
        let agent_id_to_remove = AgentID::try_from("infra-agent").unwrap();
        let mut new_sub_agents = old_sub_agents.clone();
        new_sub_agents.remove(&agent_id_to_remove);

        let diff: Vec<_> = sub_agents_difference(&old_sub_agents, &new_sub_agents).collect();

        assert_eq!(diff.len(), 1);
        assert_eq!(diff.first().unwrap().0, &agent_id_to_remove);
    }

    #[test]
    fn test_sub_agent_removal_diff_empty_new_agents() {
        let old_sub_agents = helper_get_agent_list();

        let new_sub_agents = HashMap::new();

        let diff: Vec<_> = sub_agents_difference(&old_sub_agents, &new_sub_agents).collect();

        assert_eq!(diff.len(), 2);
        assert!(diff.contains(&(
            &AgentID::try_from("infra-agent").unwrap(),
            &SubAgentConfig {
                agent_type:
                    AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1").unwrap(),
            },
        )));
        assert!(
            diff.contains(&(
                &AgentID::try_from("nrdot").unwrap(),
                &SubAgentConfig {
                    agent_type: AgentTypeID::try_from(
                        "newrelic/com.newrelic.opentelemetry.collector:0.0.1"
                    )
                    .unwrap(),
                },
            ))
        );
    }

    #[test]
    fn test_sub_agent_removal_diff_empty_old_agents() {
        let old_sub_agents = HashMap::new();

        let new_sub_agents = helper_get_agent_list();

        let diff: Vec<_> = sub_agents_difference(&old_sub_agents, &new_sub_agents).collect();

        assert!(diff.is_empty());
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Tests for TryFrom<OpampRemoteConfig> for AgentControlDynamicConfig
    ////////////////////////////////////////////////////////////////////////////////////

    #[rstest]
    #[case::single_config_multiple_agents(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}, \"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}}"}"#,
        r#"{"agents": {"agent-a": {"agent_type": "foo/bar:0.0.1"}, "agent-b": {"agent_type": "foo/bar:0.0.1"}}}"#
    )]
    #[case::multiple_configs_different_agents(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}}", "config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}}"}"#,
        r#"{"agents": {"agent-a": {"agent_type": "foo/bar:0.0.1"}, "agent-b": {"agent_type": "foo/bar:0.0.1"}}}"#
    )]
    #[case::config_with_both_versions(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"chart_version\": \"1\", \"cd_chart_version\": \"2\"}"}"#,
        r#"{"agents": {"agent-a": {"agent_type": "foo/bar:0.0.1"}}, "chart_version": "1", "cd_chart_version": "2"}"#
    )]
    #[case::multiple_configs_split_versions(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"chart_version\": \"1\"}", "config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"cd_chart_version\": \"2\"}"}"#,
        r#"{"agents": {"agent-a": {"agent_type": "foo/bar:0.0.1"}, "agent-b": {"agent_type": "foo/bar:0.0.1"}}, "chart_version": "1", "cd_chart_version": "2"}"#
    )]
    #[case::config_with_versions_and_another_with_agent(
        r#"{"config1": "{\"agents\": {}, \"chart_version\": \"1\", \"cd_chart_version\": \"2\"}", "config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}}"}"#,
        r#"{"agents": {"agent-b": {"agent_type": "foo/bar:0.0.1"}}, "chart_version": "1", "cd_chart_version": "2"}"#
    )]
    #[case::empty_agents(r#"{"config1": "{\"agents\": {}}"}"#, r#"{"agents": {}}"#)]
    fn test_opamp_remote_config_to_dynamic_config_success(
        #[case] config_json: &str,
        #[case] expected_json: &str,
    ) {
        let map: HashMap<String, String> =
            serde_json::from_str::<HashMap<String, String>>(config_json)
                .unwrap()
                .into_iter()
                .map(|(k, v)| (format!("{}-{}", AGENT_CONFIG_PREFIX, k), v))
                .collect();
        let config_map = ConfigurationMap::new(map);

        let opamp_config = OpampRemoteConfig::new(
            AgentID::try_from("test-agent").unwrap(),
            Hash::default(),
            crate::opamp::remote_config::hash::ConfigState::Applying,
            config_map,
        );

        let result = AgentControlDynamicConfig::try_from(&opamp_config);

        assert!(
            result.is_ok(),
            "Expected success but got error: {:?}",
            result.err()
        );
        let config = result.unwrap();

        let expected: AgentControlDynamicConfig = serde_json::from_str(expected_json).unwrap();
        assert_eq!(config, expected);
    }

    #[test]
    fn test_opamp_remote_config_ignores_non_agent_config_entries() {
        let mut map = HashMap::new();
        // Valid agent config entries
        map.insert(
            format!("{}-config1", AGENT_CONFIG_PREFIX),
            r#"{"agents": {"agent-a": {"agent_type": "foo/bar:0.0.1"}}}"#.to_string(),
        );
        // Invalid entries (not starting with AGENT_CONFIG_PREFIX)
        map.insert(
            "some-other-prefix-config2".to_string(),
            r#"{"agents": {"agent-b": {"agent_type": "foo/bar:0.0.1"}}}"#.to_string(),
        );

        let config_map = ConfigurationMap::new(map);

        let opamp_config = OpampRemoteConfig::new(
            AgentID::try_from("test-agent").unwrap(),
            Hash::default(),
            crate::opamp::remote_config::hash::ConfigState::Applying,
            config_map,
        );

        let result = AgentControlDynamicConfig::try_from(&opamp_config);

        assert!(
            result.is_ok(),
            "Expected success but got error: {:?}",
            result.err()
        );
        let config = result.unwrap();

        // Only agent-a should be present, agent-b and agent-c should be ignored
        assert_eq!(config.agents.len(), 1);
        assert!(
            config
                .agents
                .contains_key(&AgentID::try_from("agent-a").unwrap())
        );
        assert!(
            !config
                .agents
                .contains_key(&AgentID::try_from("agent-b").unwrap())
        );
    }

    #[rstest]
    #[case::duplicate_agent_id(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}}", "config2": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}}"}"#
    )]
    #[case::duplicate_chart_version(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"chart_version\": \"1\"}", "config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"chart_version\": \"2\"}"}"#
    )]
    #[case::duplicate_cd_chart_version(
        r#"{"config1": "{\"agents\": {\"agent-a\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"cd_chart_version\": \"1\"}", "config2": "{\"agents\": {\"agent-b\": {\"agent_type\": \"foo/bar:0.0.1\"}}, \"cd_chart_version\": \"2\"}"}"#
    )]
    fn test_opamp_remote_config_to_dynamic_config_error(#[case] config_json: &str) {
        let map: HashMap<String, String> =
            serde_json::from_str::<HashMap<String, String>>(config_json)
                .unwrap()
                .into_iter()
                .map(|(k, v)| (format!("{}-{}", AGENT_CONFIG_PREFIX, k), v))
                .collect();
        let config_map = ConfigurationMap::new(map);

        let opamp_config = OpampRemoteConfig::new(
            AgentID::try_from("test-agent").unwrap(),
            Hash::default(),
            crate::opamp::remote_config::hash::ConfigState::Applying,
            config_map,
        );

        let result = AgentControlDynamicConfig::try_from(&opamp_config);
        assert_matches!(result, Err(AgentControlConfigError(_)));
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////

    pub fn infra_identity() -> AgentIdentity {
        let id = AgentID::try_from("infra-agent").unwrap();
        let agent_type_id =
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1").unwrap();
        AgentIdentity { id, agent_type_id }
    }

    fn infra() -> HashMap<AgentID, SubAgentConfig> {
        let identity = infra_identity();
        HashMap::from([(
            identity.id,
            SubAgentConfig {
                agent_type: identity.agent_type_id,
            },
        )])
    }

    pub fn nrdot_identity() -> AgentIdentity {
        let id = AgentID::try_from("nrdot").unwrap();
        let agent_type_id =
            AgentTypeID::try_from("newrelic/com.newrelic.opentelemetry.collector:0.0.1").unwrap();
        AgentIdentity { id, agent_type_id }
    }

    fn nrdot() -> HashMap<AgentID, SubAgentConfig> {
        let identity = nrdot_identity();
        HashMap::from([(
            identity.id,
            SubAgentConfig {
                agent_type: identity.agent_type_id,
            },
        )])
    }

    pub fn helper_get_agent_list() -> HashMap<AgentID, SubAgentConfig> {
        let mut agents = HashMap::new();
        agents.extend(infra());
        agents.extend(nrdot());
        agents
    }
}
