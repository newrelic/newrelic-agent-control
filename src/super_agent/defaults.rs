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
pub const VALUES_FILENAME: &str = "values.yml";
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
        args: "--config=${config_file}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5s
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
    default: ""
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
deployment:
  on_host:
    executables:
      - path: /usr/local/bin/newrelic-infra
        args: "--config=${config_agent}"
        env: "NRIA_PLUGIN_DIR=${config_ohis} NRIA_LOGGING_CONFIGS_DIR=${logging}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5s
"#;

// NRDOT AgentType
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
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${config_file} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: "OTEL_EXPORTER_OTLP_ENDPOINT=${otel_exporter_otlp_endpoint} NEW_RELIC_MEMORY_LIMIT_MIB=${new_relic_memory_limit_mib}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5s
"#;

// Kubernetes AgentType
pub(crate) const KUBERNETES_TYPE: &str = r#"
namespace: newrelic
name: io.k8s.opentelemetry.collector # Changed to avoid collisions with the upper agent type
version: 0.0.1
variables:
  config_file:
    description: "Newrelic otel collector configuration path"
    type: yaml
    required: true
deployment:
  k8s:
    objects:
      repository:
        apiVersion: source.toolkit.fluxcd.io/v1beta2
        kind: HelmRepository
        metadata:
          labels:
            extralabel: ${extralabel}
        spec:
          interval: 3m
          url: https://open-telemetry.github.io/opentelemetry-helm-charts
      release:
        apiVersion: helm.toolkit.fluxcd.io/v2beta1
        kind: HelmRelease
        metadata:
          labels:
            extralabel: ${extralabel}
        spec:
          interval: 3m
          chart:
            spec:
              chart: opentelemetry-collector
              version: 0.67.0
              sourceRef:
                kind: HelmRepository
                name: open-telemetry # Needed this reference from above. Do not override or override above too.
                namespace: default # This comes from the static config, we need some way to inject it.
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
            mode: deployment
            config: ${config_file}
"#;

#[cfg(test)]
mod test {
    use crate::config::agent_type::agent_types::FinalAgent;

    #[test]
    fn test_parsable_configs() {
        let _: FinalAgent = serde_yaml::from_str(super::NEWRELIC_INFRA_TYPE_1).unwrap();
        let _: FinalAgent = serde_yaml::from_str(super::NEWRELIC_INFRA_TYPE_2).unwrap();
        let _: FinalAgent = serde_yaml::from_str(super::NRDOT_TYPE).unwrap();
        let _: FinalAgent = serde_yaml::from_str(super::KUBERNETES_TYPE).unwrap();
    }
}
