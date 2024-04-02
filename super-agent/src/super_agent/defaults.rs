use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;

pub const SUPER_AGENT_ID: &str = "super-agent";
pub const SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub const SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Paths
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        pub const SUB_AGENT_DIRECTORY: &str = "agents.d";
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent";
        pub const SUPER_AGENT_IDENTIFIERS_PATH: &str = "/opt/homebrew/var/lib/newrelic-super-agent/identifiers.yaml";
        pub const REMOTE_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent/fleet/agents.d";
        pub const LOCAL_AGENT_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent/fleet/agents.d";
        pub const VALUES_DIR: &str = "values";
        pub const VALUES_FILE: &str = "values.yaml";
        pub const SUPER_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent";
        pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
        pub const DYNAMIC_AGENT_TYPE :&str = "/opt/homebrew/etc/newrelic-super-agent/dynamic-agent-type.yaml";

        // Logging constants
        pub const SUPER_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
        pub const SUB_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent/fleet/agents.d";
        pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
        pub const STDERR_LOG_PREFIX: &str = "stderr.log";
    }else{
        pub const SUB_AGENT_DIRECTORY: &str = "agents.d";
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
        pub const SUPER_AGENT_IDENTIFIERS_PATH: &str = "/var/lib/newrelic-super-agent/identifiers.yaml";
        pub const REMOTE_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent/fleet/agents.d";
        pub const LOCAL_AGENT_DATA_DIR: &str = "/etc/newrelic-super-agent/fleet/agents.d";
        pub const VALUES_DIR: &str = "values";
        pub const VALUES_FILE: &str = "values.yaml";
        pub const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
        pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
        pub const DYNAMIC_AGENT_TYPE :&str = "/etc/newrelic-super-agent/dynamic-agent-type.yaml";

        // Logging constants
        pub const SUPER_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
        pub const SUB_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent/fleet/agents.d";
        pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
        pub const STDERR_LOG_PREFIX: &str = "stderr.log";
    }
}

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
pub(crate) const NEWRELIC_INFRA_TYPE_0_0_1: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.1
variables:
  on_host:
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
pub(crate) const NEWRELIC_INFRA_TYPE_0_0_2: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.2
variables:
  on_host:
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
pub(crate) const NEWRELIC_INFRA_TYPE_0_1_0: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.1.0
variables:
  on_host:
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

// Infrastructure_agent AgentType
pub(crate) const NEWRELIC_INFRA_TYPE_0_1_1: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.1.1
variables:
  on_host:
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
    enable_file_logging:
      description: "enable logging the on host executables' logs to files"
      type: bool
      required: false
      default: false
deployment:
  on_host:
    enable_file_logging: ${nr-var:enable_file_logging}
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${nr-var:config_agent}"
        env: "NRIA_PLUGIN_DIR=${nr-var:config_integrations} NRIA_LOGGING_CONFIGS_DIR=${nr-var:config_logging}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
"#;

pub(crate) const NEWRELIC_INFRA_TYPE_0_1_2: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.1.1
variables:
  on_host:
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
    enable_file_logging:
      description: "enable logging the on host executables' logs to files"
      type: bool
      required: false
      default: false
deployment:
  on_host:
    enable_file_logging: ${nr-var:enable_file_logging}
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${nr-var:config_agent}"
        env: "NRIA_PLUGIN_DIR=${nr-var:config_integrations} NRIA_LOGGING_CONFIGS_DIR=${nr-var:config_logging} NRIA_STATUS_SERVER_ENABLED=true"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
        health:
          interval: 5s
          timeout: 5s
          http:
            path: "/v1/status"
            port: 8003
"#;

// NRDOT AgentType
pub(crate) const NRDOT_TYPE_0_0_1: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector
version: 0.0.1
variables:
  on_host:
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
  k8s:
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
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${nr-var:config_file} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: "OTEL_EXPORTER_OTLP_ENDPOINT=${nr-var:otel_exporter_otlp_endpoint} NEW_RELIC_MEMORY_LIMIT_MIB=${nr-var:new_relic_memory_limit_mib}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
  k8s:
    objects:
      repository:
        apiVersion: source.toolkit.fluxcd.io/v1beta2
        kind: HelmRepository
        metadata:
          name: ${nr-sub:agent_id}
        spec:
          # Increasing the interval to 30 minutes because the HelmRepository is not as prone to frequent changes
          # as HelmRelease objects might be. Given repositories typically have fewer updates than the resources
          # they trigger, a longer interval helps in reducing unnecessary load on the cluster without significantly
          # delaying the application of important updates.
          interval: 30m
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
            # Wait are disabled to avoid blocking the modifications/deletions of this CR while in reconciling state.
            disableWait: true
            disableWaitForJobs: true
            remediation:
              retries: 3
            replace: true
          upgrade:
            disableWait: true
            disableWaitForJobs: true
            cleanupOnFail: true
            force: true
            remediation:
              retries: 3
              strategy: rollback
          rollback:
            disableWait: true
            disableWaitForJobs: true
          values:
            ${nr-var:chart_values}
"#;

// NRDOT AgentType
pub(crate) const NRDOT_TYPE_0_1_0: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector
version: 0.1.0
variables:
  on_host:
    config:
      description: "Newrelic otel collector configuration"
      type: file
      required: false
      default: ""
      file_path: "config.yaml"
    backoff_delay:
      description: "seconds until next retry if agent fails to start"
      type: string
      required: false
      default: 20s
  k8s:
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
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${nr-var:config} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: ""
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${nr-var:backoff_delay}
  k8s:
    objects:
      repository:
        apiVersion: source.toolkit.fluxcd.io/v1beta2
        kind: HelmRepository
        metadata:
          name: ${nr-sub:agent_id}
        spec:
          # Increasing the interval to 30 minutes because the HelmRepository is not as prone to frequent changes
          # as HelmRelease objects might be. Given repositories typically have fewer updates than the resources
          # they trigger, a longer interval helps in reducing unnecessary load on the cluster without significantly
          # delaying the application of important updates.
          interval: 30m
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
            # Wait are disabled to avoid blocking the modifications/deletions of this CR while in reconciling state.
            disableWait: true
            disableWaitForJobs: true
            remediation:
              retries: 3
            replace: true
          upgrade:
            disableWait: true
            disableWaitForJobs: true
            cleanupOnFail: true
            force: true
            remediation:
              retries: 3
              strategy: rollback
          rollback:
            disableWait: true
            disableWaitForJobs: true
          values:
            ${nr-var:chart_values}
"#;

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        agent_type::{definition::AgentTypeDefinition, environment::Environment},
        sub_agent::effective_agents_assembler::build_agent_type,
    };

    #[test]
    fn test_parsable_configs() {
        let yaml_definitions = vec![
            NEWRELIC_INFRA_TYPE_0_0_1,
            NEWRELIC_INFRA_TYPE_0_0_2,
            NEWRELIC_INFRA_TYPE_0_1_0,
            NEWRELIC_INFRA_TYPE_0_1_1,
            NEWRELIC_INFRA_TYPE_0_1_2,
            NRDOT_TYPE_0_0_1,
            NRDOT_TYPE_0_1_0,
        ];

        for yaml in yaml_definitions {
            let definition = serde_yaml::from_str::<AgentTypeDefinition>(yaml).unwrap();
            build_agent_type(definition.clone(), &Environment::K8s).unwrap();
            build_agent_type(definition, &Environment::OnHost).unwrap();
        }
    }
}
