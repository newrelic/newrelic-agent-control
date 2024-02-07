use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;

pub const SUPER_AGENT_ID: &str = "super-agent";
pub const SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub const SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Paths

pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
pub const SUPER_AGENT_IDENTIFIERS_PATH: &str = "/var/lib/newrelic-super-agent/identifiers.yaml";
pub const REMOTE_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent/fleet/agents.d";
pub const LOCAL_AGENT_DATA_DIR: &str = "/etc/newrelic-super-agent/fleet/agents.d";
pub const VALUES_DIR: &str = "values";
pub const VALUES_FILE: &str = "values.yaml";
pub const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
pub const GENERATED_FOLDER_NAME: &str = "auto-generated";

pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}

// Infrastructure_agent AgentType
pub(crate) const NEWRELIC_INFRA_TYPE_1: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.1
variables:
  config_file:
    description: "Newrelic infra configuration path"
    type: string
    required: false
    default: /etc/newrelic-infra.yml
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${nr-var:config_file}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: 20s
"#;

// Infrastructure_agent AgentType
pub(crate) const NEWRELIC_INFRA_TYPE_2: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.2
variables:
  config_agent:
    description: "Newrelic infra configuration"
    type: file
    required: false
    default: |
      "content"
    file_path: "newrelic-infra.yml"
  config_ohis:
    description: "map of YAML configs for the OHIs"
    type: map[string]file
    required: false
    default: {}
    file_path: "integrations.d"
  logging:
    description: "map of YAML config for logging"
    type: map[string]file
    required: false
    default: {}
    file_path: "logging.d"
  backoff_delay:
    description: "seconds until next retry if agent fails to start"
    type: string
    required: false
    default: 20s
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${nr-var:config_agent}"
        env: "NRIA_PLUGIN_DIR=${nr-var:config_ohis} NRIA_LOGGING_CONFIGS_DIR=${nr-var:logging}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
"#;

// Infrastructure_agent AgentType
pub(crate) const NEWRELIC_INFRA_TYPE_3: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.1.0
variables:
  config_agent:
    description: "Newrelic infra configuration"
    type: file
    required: false
    default: ""
    file_path: "newrelic-infra.yml"
  config_integrations:
    description: "map of YAML configs for the OHIs"
    type: map[string]file
    required: false
    default: {}
    file_path: "integrations.d"
  config_logging:
    description: "map of YAML config for logging"
    type: map[string]file
    required: false
    default: {}
    file_path: "logging.d"
  backoff_delay:
    description: "seconds until next retry if agent fails to start"
    type: string
    required: false
    default: 20s
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${nr-var:config_agent}"
        env: "NRIA_PLUGIN_DIR=${nr-var:config_integrations} NRIA_LOGGING_CONFIGS_DIR=${nr-var:config_logging}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
"#;

// NRDOT AgentType
#[cfg(feature = "onhost")]
pub(crate) const NRDOT_TYPE: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector
version: 0.0.1
variables:
  config_file:
    description: "Newrelic otel collector configuration path"
    type: string
    required: false
    default: /etc/nr-otel-collector/config.yaml
  otel_exporter_otlp_endpoint:
    description: "Endpoint where NRDOT will send data"
    type: string
    required: false
    default: "otlp.nr-data.net:4317"
  new_relic_memory_limit_mib:
    description: "Memory limit for the NRDOT process"
    type: number
    required: false
    default: 100
  backoff_delay:
    description: "seconds until next retry if agent fails to start"
    type: string
    required: false
    default: 20s
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${nr-var:config_file} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: "OTEL_EXPORTER_OTLP_ENDPOINT=${nr-var:otel_exporter_otlp_endpoint} NEW_RELIC_MEMORY_LIMIT_MIB=${nr-var:new_relic_memory_limit_mib}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
"#;

// Kubernetes AgentType
// TODO We need to unify the two agent types and remove this workaround
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub(crate) const NRDOT_TYPE: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector 
version: 0.0.1
variables:
  chart_values:
    description: "Newrelic otel collector chart values"
    type: yaml
    required: true
  chart_version:
    description: "Newrelic otel collector chart version"
    type: string
    required: true
    default: "0.78.3"
deployment:
  k8s:
    objects:
      repository:
        apiVersion: source.toolkit.fluxcd.io/v1beta2
        kind: HelmRepository
        metadata:
          name: ${nr-sub:agent_id}
        spec:
          interval: 3m
          url: https://open-telemetry.github.io/opentelemetry-helm-charts
      release:
        apiVersion: helm.toolkit.fluxcd.io/v2beta2
        kind: HelmRelease
        metadata:
          name: ${nr-sub:agent_id}
        spec:
          interval: 3m
          chart:
            spec:
              chart: opentelemetry-collector
              version: ${nr-var:chart_version}
              sourceRef:
                kind: HelmRepository
                name: ${nr-sub:agent_id}
              interval: 3m
          install:
            remediation:
              retries: 3
            replace: true
          upgrade:
            cleanupOnFail: true
            force: true
            remediation:
              retries: 3
              strategy: rollback
          values:
            ${nr-var:chart_values}
"#;

#[cfg(test)]
mod test {
    use crate::agent_type::definition::AgentType;

    #[test]
    fn test_parsable_configs() {
        serde_yaml::from_str::<AgentType>(super::NEWRELIC_INFRA_TYPE_1).unwrap();
        serde_yaml::from_str::<AgentType>(super::NEWRELIC_INFRA_TYPE_2).unwrap();
        serde_yaml::from_str::<AgentType>(super::NRDOT_TYPE).unwrap();
    }
}
