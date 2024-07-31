use std::collections::HashMap;

use regex::Regex;
use thiserror::Error;

use crate::opamp::remote_config::RemoteConfig;
use crate::sub_agent::validation_regexes::{
    REGEX_BINARY_PATH_FIELD, REGEX_COMMAND_FIELD, REGEX_EXEC_FIELD, REGEX_NRI_FLEX,
    REGEX_OTEL_ENDPOINT, REGEX_VALID_OTEL_ENDPOINT,
};
use crate::super_agent::config::AgentTypeFQN;

pub const FQN_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure_agent";
pub const FQN_NAME_NRDOT: &str = "io.opentelemetry.collector";

#[derive(Error, Debug)]
pub enum ValidatorError {
    #[error("Invalid config: restricted values detected")]
    InvalidConfig,

    #[error("error compiling regex: `{0}`")]
    RegexError(#[from] regex::Error),
}

#[derive(Debug, PartialEq, Hash, Eq)]
pub(super) struct AgentTypeFQNName(String);

/// The Config validator is responsible for matching a series of regexes on the content
/// of the retrieved remote config and returning an error if a match is found.
/// If getting the unique remote config fails, the validator will return as valid
/// because we leave that kind of error handling to the store_remote_config_hash_and_values
/// on the event_processor.
pub struct ConfigValidator {
    rules: HashMap<AgentTypeFQNName, Vec<Regex>>,

    // regex to match any endpoint field in the nrdot config
    otel_endpoint: Regex,
    // regex to match a valid newrelic otel endpoint
    valid_otel_endpoint: Regex,
}

impl ConfigValidator {
    pub fn try_new() -> Result<Self, ValidatorError> {
        Ok(Self {
            rules: HashMap::from([(
                AgentTypeFQNName(FQN_NAME_INFRA_AGENT.to_string()),
                vec![
                    Regex::new(REGEX_COMMAND_FIELD)?,
                    Regex::new(REGEX_EXEC_FIELD)?,
                    Regex::new(REGEX_BINARY_PATH_FIELD)?,
                    Regex::new(REGEX_NRI_FLEX)?,
                ],
            )]),
            otel_endpoint: Regex::new(REGEX_OTEL_ENDPOINT)?,
            valid_otel_endpoint: Regex::new(REGEX_VALID_OTEL_ENDPOINT)?,
        })
    }
    pub fn validate(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        remote_config: &RemoteConfig,
    ) -> Result<(), ValidatorError> {
        // This config will fail further on the event processor.
        if let Ok(raw_config) = remote_config.get_unique() {
            self.validate_regex_rules(agent_type_fqn, raw_config)?;
            self.validate_nrdot_endpoint(agent_type_fqn, raw_config)?;
        }

        Ok(())
    }

    fn validate_regex_rules(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        raw_config: &str,
    ) -> Result<(), ValidatorError> {
        let agent_type_fqn_name = AgentTypeFQNName(agent_type_fqn.name());
        if !self.rules.contains_key(&agent_type_fqn_name) {
            return Ok(());
        }

        for regex in self.rules[&agent_type_fqn_name].iter() {
            if regex.is_match(raw_config) {
                return Err(ValidatorError::InvalidConfig);
            }
        }

        Ok(())
    }

    /// Validates all 'endpoint' in the nrdot config contains a valid newrelic otel endpoint.
    fn validate_nrdot_endpoint(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        raw_config: &str,
    ) -> Result<(), ValidatorError> {
        // this rule applies only to nrdot agents
        if !agent_type_fqn.name().eq(FQN_NAME_NRDOT) {
            return Ok(());
        }

        // gathers all the endpoints in the config
        for capture in self.otel_endpoint.captures_iter(raw_config) {
            if let Some(endpoint) = capture.get(1) {
                // verifies that the endpoint is valid
                if !self.valid_otel_endpoint.is_match(endpoint.as_str()) {
                    return Err(ValidatorError::InvalidConfig);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
pub(super) mod test {
    use std::collections::HashMap;

    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::{ConfigValidator, FQN_NAME_INFRA_AGENT};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};

    #[test]
    fn test_validate() {
        let content = r#"
        health_port: 18003
        config_agent: |+
          staging: true
          enable_process_metrics: true
          status_server_enabled: true
          status_server_port: 18003
          log:
            level: info
          license_key: {{NEW_RELIC_LICENSE_KEY}}
          custom_attributes:
            nr_deployed_by: newrelic-cli

        config_integrations:
          docker-config.yml: |
            integrations:
              - name: nri-docker
                when:
                  feature: docker_enabled
                  file_exists: /var/run/docker.sock
                interval: 15s
              # This configuration is no longer included in nri-ecs images.
              # it is kept for legacy reasons, but the new one is located in https://github.com/newrelic/nri-ecs
              - name: nri-docker
                when:
                  feature: docker_enabled
                  env_exists:
                    FARGATE: "true"
                interval: 15s
                        integrations:
              - name: nri-other
                exec: /bin/crowdstrike-falcon
                interval: 15s
        "#;
        let remote_config = RemoteConfig::new(
            AgentID::new("invented").unwrap(),
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                content.to_string(),
            )]))),
        );
        let validator = ConfigValidator::try_new().unwrap();
        let agent_type_fqn =
            AgentTypeFQN::try_from(format!("newrelic/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();
        let validation_result = validator.validate(&agent_type_fqn, &remote_config);
        assert_eq!(
            validation_result.unwrap_err().to_string(),
            "Invalid config: restricted values detected"
        );
    }

    #[test]
    fn nrdot_endpoint() {
        struct TestCase {
            name: &'static str,
            config: &'static str,
            valid: bool,
        }
        impl TestCase {
            fn run(self) {
                let remote_config = RemoteConfig::new(
                    AgentID::new("fake").unwrap(),
                    Hash::new("fake".to_string()),
                    Some(ConfigurationMap::new(HashMap::from([(
                        "".to_string(),
                        self.config.to_string(),
                    )]))),
                );

                let agent_type_fqn =
                    AgentTypeFQN::try_from("newrelic/io.opentelemetry.collector:9.9.9").unwrap();

                let validator = ConfigValidator::try_new().unwrap();

                let res = validator.validate(&agent_type_fqn, &remote_config);
                assert_eq!(res.is_ok(), self.valid, "test case: {}", self.name);
            }
        }

        let test_cases = vec![
            // valid cases
            TestCase {
                name: "valid real config",
                config: VALID_REAL_CONFIG_1,
                valid: true,
            },
            TestCase {
                name: "valid single endpoint",
                config: r#"
config: |
  exporters:
    otlp/nr:
      endpoint: "https://otlp.nr-data.net:4317"
"#,
                valid: true,
            },
            TestCase {
                name: "valid single endpoint without quotes",
                config: r#"
config: |
  exporters:
    otlp/nr:
      endpoint: https://otlp.nr-data.net:4317
"#,
                valid: true,
            },
            TestCase {
                name: "all valid combination endpoints",
                config: r#"
  endpoint : otlp.nr-data.net:4317
  endpoint : staging-otlp.nr-data.net:1234
  endpoint : otlp.eu01.nr-data.net:443
  endpoint : https://otlp.nr-data.net:4317
  endpoint : https://staging-otlp.nr-data.net:1234
  endpoint : https://otlp.eu01.nr-data.net:443
  endpoint : ${OTEL_EXPORTER_OTLP_ENDPOINT}
  endpoint : "otlp.nr-data.net:4317"
  endpoint : "staging-otlp.nr-data.net:1234"
  endpoint : "otlp.eu01.nr-data.net:443"
  endpoint : "https://otlp.nr-data.net:4317"
  endpoint : "https://staging-otlp.nr-data.net:1234"
  endpoint : "https://otlp.eu01.nr-data.net:443"
  endpoint : "${OTEL_EXPORTER_OTLP_ENDPOINT}"
"#,
                valid: true,
            },
            // invalid cases
            TestCase {
                name: "invalid single endpoint",
                config: r#"
config: |
exporters:
  otlp/nr:
    endpoint: "https://my-server:4317"
"#,
                valid: false,
            },
            TestCase {
                name: "invalid suffix",
                config: r#"
endpoint: https://otlp.nr-data.net-fake:4317
"#,
                valid: false,
            },
            TestCase {
                name: "invalid prefix",
                config: r#"
endpoint: https://fake-otlp.nr-data.net:4317
"#,
                valid: false,
            },
            TestCase {
                name: "invalid with spaces",
                config: r#"
endpoint :   my-fake-server:4317
"#,
                valid: false,
            },
            TestCase {
                name: "multiple endpoints with one invalid",
                config: r#"
config: |
exporters:
  otlp/nr:
    endpoint: "https://otlp.nr-data.net:4317"
  otlp/test:
    endpoint: "https://my-server:4317"
"#,
                valid: false,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    static VALID_REAL_CONFIG_1: &str = r#"
config: |

  extensions:
    health_check:

  receivers:
    otlp:
      protocols:
        grpc:
        http:

    hostmetrics:
      collection_interval: 20s
      scrapers:
        cpu:
          metrics:
            system.cpu.time:
              enabled: false
            system.cpu.utilization:
              enabled: true
        load:
        memory:
          metrics:
            system.memory.utilization:
              enabled: true
        paging:
          metrics:
            system.paging.utilization:
              enabled: false
            system.paging.faults:
              enabled: false
        filesystem:
          metrics:
            system.filesystem.utilization:
              enabled: true
        disk:
          metrics:
            system.disk.merged:
              enabled: false
            system.disk.pending_operations:
              enabled: false
            system.disk.weighted_io_time:
              enabled: false
        network:
          metrics:
            system.network.connections:
              enabled: false
        processes:
        process:
          metrics:
            process.cpu.utilization:
              enabled: true
            process.cpu.time:
              enabled: false

    filelog:
      include:
        - /var/log/alternatives.log
        - /var/log/cloud-init.log
        - /var/log/auth.log
        - /var/log/dpkg.log
        - /var/log/syslog
        - /var/log/messages
        - /var/log/secure
        - /var/log/yum.log

  processors:
    # group system.cpu metrics by cpu
    metricstransform:
      transforms:
        - include: system.cpu.utilization
          action: update
          operations:
            - action: aggregate_labels
              label_set: [ state ]
              aggregation_type: mean
        - include: system.paging.operations
          action: update
          operations:
            - action: aggregate_labels
              label_set: [ direction ]
              aggregation_type: sum
    # remove system.cpu metrics for states
    filter/exclude_cpu_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "interrupt"'
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "nice"'
          - 'metric.name == "system.cpu.utilization" and attributes["state"] == "softirq"'
    filter/exclude_memory_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "slab_unreclaimable"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "inactive"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "cached"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "buffered"'
          - 'metric.name == "system.memory.utilization" and attributes["state"] == "slab_reclaimable"'
    filter/exclude_memory_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.memory.usage" and attributes["state"] == "slab_unreclaimable"'
          - 'metric.name == "system.memory.usage" and attributes["state"] == "inactive"'
    filter/exclude_filesystem_utilization:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.utilization" and attributes["type"] == "squashfs"'
    filter/exclude_filesystem_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.usage" and attributes["type"] == "squashfs"'
          - 'metric.name == "system.filesystem.usage" and attributes["state"] == "reserved"'
    filter/exclude_filesystem_inodes_usage:
      metrics:
        datapoint:
          - 'metric.name == "system.filesystem.inodes.usage" and attributes["type"] == "squashfs"'
          - 'metric.name == "system.filesystem.inodes.usage" and attributes["state"] == "reserved"'
    filter/exclude_system_disk:
      metrics:
        datapoint:
          - 'metric.name == "system.disk.operations" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.merged" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.io" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.io_time" and IsMatch(attributes["device"], "^loop.*") == true'
          - 'metric.name == "system.disk.operation_time" and IsMatch(attributes["device"], "^loop.*") == true'
    filter/exclude_system_paging:
      metrics:
        datapoint:
          - 'metric.name == "system.paging.usage" and attributes["state"] == "cached"'
          - 'metric.name == "system.paging.operations" and attributes["type"] == "cached"'
    filter/exclude_network:
      metrics:
        datapoint:
          - 'IsMatch(metric.name, "^system.network.*") == true and attributes["device"] == "lo"'

    attributes/exclude_system_paging:
      include:
        match_type: strict
        metric_names:
          - system.paging.operations
      actions:
        - key: type
          action: delete

    transform:
      trace_statements:
        - context: span
          statements:
            - truncate_all(attributes, 4095)
            - truncate_all(resource.attributes, 4095)
      log_statements:
        - context: log
          statements:
            - truncate_all(attributes, 4095)
            - truncate_all(resource.attributes, 4095)

    # used to prevent out of memory situations on the collector
    memory_limiter:
      check_interval: 1s
      limit_mib: 100

    batch:

    resource:
      attributes:
        - key: host.display_name
          action: upsert
          value: {{ display_name }}

    resourcedetection:
      detectors: ["env", "system"]
      system:
        hostname_sources: ["os"]
        resource_attributes:
          host.id:
            enabled: true

    resourcedetection/cloud:
      detectors: ["gcp", "ec2", "azure"]
      timeout: 2s
      ec2:
        resource_attributes:
          host.name:
            enabled: false

  exporters:
    logging:
    otlp:
      endpoint: staging-otlp.nr-data.net:4317
      headers:
        api-key: {{ nr_license_key_canaries }}

  service:
"#;
}
