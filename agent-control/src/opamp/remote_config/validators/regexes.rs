use super::RemoteConfigValidator;
use crate::agent_control::defaults::{AGENT_TYPE_NAME_INFRA_AGENT, AGENT_TYPE_NAME_NRDOT};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::sub_agent::identity::AgentIdentity;
use regex::Regex;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegexValidatorError {
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
pub struct RegexValidator {
    rules: HashMap<AgentTypeFQNName, Vec<Regex>>,

    // regex to match any repository field in the nrdot config
    otel_repository: Regex,
}

impl RemoteConfigValidator for RegexValidator {
    type Err = RegexValidatorError;
    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        opamp_remote_config: &OpampRemoteConfig,
    ) -> Result<(), RegexValidatorError> {
        // This config will fail further on the event processor.
        if let Ok(raw_config) = opamp_remote_config.get_unique() {
            self.validate_regex_rules(&agent_identity.agent_type_id, raw_config)?;
            self.validate_nrdot_repository(&agent_identity.agent_type_id, raw_config)?;
        }

        Ok(())
    }
}

impl RegexValidator {
    fn try_new() -> Result<Self, RegexValidatorError> {
        Ok(Self {
            rules: HashMap::from([(
                AgentTypeFQNName(AGENT_TYPE_NAME_INFRA_AGENT.to_string()),
                vec![
                    Regex::new(REGEX_COMMAND_FIELD)?,
                    Regex::new(REGEX_EXEC_FIELD)?,
                    Regex::new(REGEX_BINARY_PATH_FIELD)?,
                    Regex::new(REGEX_NRI_FLEX)?,
                ],
            )]),
            otel_repository: Regex::new(REGEX_OTEL_REPOSITORY)?,
        })
    }

    fn validate_regex_rules(
        &self,
        agent_type_id: &AgentTypeID,
        raw_config: &str,
    ) -> Result<(), RegexValidatorError> {
        let agent_type_name = AgentTypeFQNName(agent_type_id.name().to_string());
        if !self.rules.contains_key(&agent_type_name) {
            return Ok(());
        }

        for regex in self.rules[&agent_type_name].iter() {
            if regex.is_match(raw_config) {
                return Err(RegexValidatorError::InvalidConfig);
            }
        }

        Ok(())
    }

    /// Validates the 'repository' in the nrdot config.
    fn validate_nrdot_repository(
        &self,
        agent_type: &AgentTypeID,
        raw_config: &str,
    ) -> Result<(), RegexValidatorError> {
        // this rule applies only to nrdot agents
        if !agent_type.name().eq(AGENT_TYPE_NAME_NRDOT) {
            return Ok(());
        }

        // gathers all the endpoints in the config
        for capture in self.otel_repository.captures_iter(raw_config) {
            if let Some(repository) = capture.get(1) {
                if VALID_OTEL_REPOSITORY != repository.as_str() {
                    return Err(RegexValidatorError::InvalidConfig);
                }
            }
        }

        Ok(())
    }
}

impl Default for RegexValidator {
    fn default() -> Self {
        // Notice that we allow an expect here since all regexes are hardcoded
        Self::try_new().expect("Failed to compile config validation regexes")
    }
}

// deny using custom images for nr-dot
// https://github.com/newrelic/helm-charts/blob/nr-k8s-otel-collector-0.7.4/charts/nr-k8s-otel-collector/values.yaml#L16
// Example:
// chart_values:
//   image:
//     repository: newrelic/nr-otel-collector
//     pullPolicy: IfNotPresent
//     tag: "0.7.1"
pub static REGEX_OTEL_REPOSITORY: &str = r"\s*repository\s*:\s*(.+)";
pub static VALID_OTEL_REPOSITORY: &str = "newrelic/nr-otel-collector";

// Infra Agent Integrations (OHI)
// deny any config for integrations that contains discovery command
// https://github.com/newrelic/infrastructure-agent/blob/1.55.1/pkg/databind/internal/discovery/command.go#L14
// Example:
//     discovery:
//       command:
//         # Use the following optional arguments:
//         # --namespaces: Comma separated list of namespaces to discover pods on
//         # --tls: Use secure (TLS) connection
//         # --port: Port used to connect to the kubelet. Default is 10255
//         exec: /var/db/newrelic-infra/nri-discovery-kubernetes
//         match:
//           label.app: mysql
//
// deny any config for the Infra Agent custom secret management command
// https://docs.newrelic.com/docs/infrastructure/host-integrations/installation/secrets-management/#custom-commands
// Example:
// variables:
//   myToken:
//     command:
//       path: '/path/to/my-custom-auth-json'
//       args: ["--domain", "myDomain", "--other_param", "otherValue"]
//       ttl: 24h
pub static REGEX_COMMAND_FIELD: &str = "command\\s*:";

// deny integrations arbitrary command execution
// https://docs.newrelic.com/docs/infrastructure/host-integrations/infrastructure-integrations-sdk/specifications/host-integrations-standard-configuration-format/#exec
// Example:
// - name: my-integration
//   exec: /usr/bin/python /opt/integrations/my-script.py --host=127.0.0.1
pub static REGEX_EXEC_FIELD: &str = "exec\\s*:";

// deny specific binary paths (i.e. nri-apache)
// https://github.com/newrelic/nri-apache/blob/v1.12.6/apache-config.yml.sample#L35
// Example:
// - name: nri-apache
//   env:
//     INVENTORY: "true"
//     # status_url is used to identify the monitored entity to which the inventory will be attached.
//     STATUS_URL: http://127.0.0.1/server-status?auto
//
//     # binary_path is used to specify the path of the apache binary file (i.e. "C:\Apache\bin\httpd.exe").
//     # By default the integration automatically discovers the binary on "/usr/sbin/httpd" or "/usr/sbin/apache2ctl". Use this setting for any other location.
//     # BINARY_PATH: ""
// (?i:exp)       case-insensitive
// (?flags:exp)   set flags for exp (non-capturing)
pub static REGEX_BINARY_PATH_FIELD: &str = "(?i:BINARY_PATH)";

// deny using nri-flex
pub static REGEX_NRI_FLEX: &str = "nri-flex";

#[cfg(test)]
pub(super) mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::{AGENT_TYPE_NAME_INFRA_AGENT, AGENT_TYPE_NAME_NRDOT};
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::validators::RemoteConfigValidator;
    use crate::opamp::remote_config::validators::regexes::{RegexValidator, RegexValidatorError};
    use crate::opamp::remote_config::{ConfigurationMap, OpampRemoteConfig};
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;
    use std::collections::HashMap;

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
        let remote_config = OpampRemoteConfig::new(
            test_id(),
            Hash::from("this-is-a-hash"),
            ConfigState::Applying,
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                content.to_string(),
            )]))),
        );
        let validator = RegexValidator::default();
        let agent_identity = AgentIdentity {
            id: test_id(),
            agent_type_id: AgentTypeID::try_from(
                format!("newrelic/{AGENT_TYPE_NAME_INFRA_AGENT}:0.0.1").as_str(),
            )
            .unwrap(),
        };

        let validation_result = validator.validate(&agent_identity, &remote_config);
        assert_eq!(
            validation_result.unwrap_err().to_string(),
            "Invalid config: restricted values detected"
        );
    }

    #[test]
    fn nrdot_repository() {
        struct TestCase {
            name: &'static str,
            config: &'static str,
            valid: bool,
        }
        impl TestCase {
            fn run(self) {
                let remote_config = OpampRemoteConfig::new(
                    test_id(),
                    Hash::from("fake"),
                    ConfigState::Applying,
                    Some(ConfigurationMap::new(HashMap::from([(
                        "".to_string(),
                        self.config.to_string(),
                    )]))),
                );

                let agent_identity = AgentIdentity {
                    id: test_id(),
                    agent_type_id: AgentTypeID::try_from(
                        "newrelic/io.opentelemetry.collector:9.9.9",
                    )
                    .unwrap(),
                };

                let validator = RegexValidator::default();

                let res = validator.validate(&agent_identity, &remote_config);
                assert_eq!(res.is_ok(), self.valid, "test case: {}", self.name);
            }
        }

        let test_cases = vec![
            // valid cases
            TestCase {
                name: "valid real config",
                config: VALID_ONHOST_NRDOT_CONFIG,
                valid: true,
            },
            TestCase {
                name: "valid repository and ignore comment",
                config: r#"
            config: |
              image:
                repository: newrelic/nr-otel-collector
                pullPolicy: IfNotPresent
                # repository: fake/fake
                tag: "0.8.3" # repository: fake/fake
            "#,
                valid: false,
                // Currently, we are not checking if the string is present in comments or not
            },
            TestCase {
                name: "no repository is valid",
                config: r#"
            config: |
              image:
                pullPolicy: IfNotPresent
                tag: "0.8.3"
            "#,
                valid: true,
            },
            TestCase {
                name: "empty is valid",
                config: r#"
            "#,
                valid: true,
            },
            TestCase {
                name: "missing namespace",
                config: r#"
config: |
  image:
    repository: nr-otel-collector
    pullPolicy: IfNotPresent
    tag: "0.8.3"
"#,
                valid: false,
            },
            TestCase {
                name: "wrong namespace",
                config: r#"
config: |
  image:
    repository: fake/nr-otel-collector
    pullPolicy: IfNotPresent
    tag: "0.8.3"
"#,
                valid: false,
            },
            TestCase {
                name: "wrong repo name",
                config: r#"
config: |
  image:
    repository: newrelic/nr-otel-collector2
    pullPolicy: IfNotPresent
    tag: "0.8.3"
"#,
                valid: false,
            },
            TestCase {
                name: "second repo is wrong",
                config: r#"
config: |
  image:
    repository: newrelic/nr-otel-collector
    pullPolicy: IfNotPresent
    tag: "0.8.3"
    different:
      repository: newrelic/fakr
"#,
                valid: false,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    pub static VALID_ONHOST_NRDOT_CONFIG: &str = r#"
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

    #[test]
    fn test_valid_configs_are_allowed() {
        let config_validator = RegexValidator::default();

        let config = remote_config(GOOD_INFRA_AGENT_CONFIG);
        let result = config_validator.validate(&infra_agent(), &config);
        assert!(result.is_ok());

        let config = remote_config(GOOD_K8S_NRDOT_CONFIG);
        let result = config_validator.validate(&nrdot(), &config);
        assert!(result.is_ok());

        let config = remote_config(VALID_ONHOST_NRDOT_CONFIG);
        let result = config_validator.validate(&nrdot(), &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_configs_are_blocked() {
        struct TestCase {
            _name: &'static str,
            agent_identity: AgentIdentity,
            config: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let config_validator = RegexValidator::default();
                let remote_config = remote_config(self.config);
                let err = config_validator
                    .validate(&self.agent_identity, &remote_config)
                    .unwrap_err();

                assert_matches!(err, RegexValidatorError::InvalidConfig);
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "infra-agent config with nri-flex should be invalid",
                agent_identity: infra_agent(),
                config: CONFIG_WITH_NRI_FLEX,
            },
            TestCase {
                _name: "infra-agent config with command should be invalid",
                agent_identity: infra_agent(),
                config: CONFIG_WITH_COMMAND,
            },
            TestCase {
                _name: "infra-agent config with exec should be invalid",
                agent_identity: infra_agent(),
                config: CONFIG_WITH_EXEC,
            },
            TestCase {
                _name: "infra-agent config with binary_path uppercase should be invalid",
                agent_identity: infra_agent(),
                config: CONFIG_WITH_BINARY_PATH_UPPERCASE,
            },
            TestCase {
                _name: "infra-agent config with binary_path lowercase should be invalid",
                agent_identity: infra_agent(),
                config: CONFIG_WITH_BINARY_PATH_LOWERCASE,
            },
            TestCase {
                _name: "nrdot config with image repository  should be invalid",
                agent_identity: nrdot(),
                config: CONFIG_WITH_IMAGE_REPOSITORY,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    ///////////////////////////////////////////////////////
    // Helpers
    ///////////////////////////////////////////////////////
    fn test_id() -> AgentID {
        AgentID::try_from("test").unwrap()
    }

    fn infra_agent() -> AgentIdentity {
        AgentIdentity {
            id: test_id(),
            agent_type_id: AgentTypeID::try_from(
                format!("newrelic/{AGENT_TYPE_NAME_INFRA_AGENT}:0.0.1").as_str(),
            )
            .unwrap(),
        }
    }

    fn nrdot() -> AgentIdentity {
        AgentIdentity {
            id: test_id(),
            agent_type_id: AgentTypeID::try_from(
                format!("newrelic/{AGENT_TYPE_NAME_NRDOT}:0.0.1").as_str(),
            )
            .unwrap(),
        }
    }

    fn remote_config(config: &str) -> OpampRemoteConfig {
        OpampRemoteConfig::new(
            test_id(),
            Hash::from("this-is-a-hash"),
            ConfigState::Applying,
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                config.to_string(),
            )]))),
        )
    }

    // config containing nri-flex integration to be denied
    const CONFIG_WITH_NRI_FLEX: &str = r#"
################################################
# Values file for Infrastructure Agent 0.1.0
################################################

# Configuration for the Infrastructure Agent
config_agent: |
  license_key: {{ NEW_RELIC_LICENSE_KEY }}
  staging: true
  display_name: host-display-name
  enable_process_metrics: true
  log:
    level: debug
    forward: true

# Configuration for New Relic Integrations
config_integrations:
  flex.yml: |
    integrations:
      - name: nri-flex
        offset: 10s
        config:
          name: RandomNumbers
          apis:
            - name: someService
              entity: someEntity
              url: https://jsonplaceholder.typicode.com/todos/1
              math:
                sum: ${id} + ${userId} + 1
  mysql.yml: |
    integrations:
      - name: nri-mysql
        env:
          HOSTNAME: the-mysql-host
          PORT: the-mysql-port
          USERNAME: ${nr-env:MYSQL_USER}
          PASSWORD: ${nr-env:MYSQL_PASSWORD}
          REMOTE_MONITORING: true
        interval: 10s
        labels:
          env: production
          role: write-replica
        inventory_source: config/mysql
"#;

    // config with `command` field to be denied
    const CONFIG_WITH_COMMAND: &str = r#"
################################################
# Values file for Infrastructure Agent 0.1.0
################################################

# Configuration for the Infrastructure Agent
config_agent: |
  license_key: {{ NEW_RELIC_LICENSE_KEY }}
  staging: true
  display_name: host-display-name
  enable_process_metrics: true
  log:
    level: debug
    forward: true

# Configuration for New Relic Integrations
config_integrations:
  mysql.yml: |
    integrations:
      - name: nri-mysql
        offset: 10s
        config:
          name: RandomNumbers
          command: an extra command
  mysql.yml: |
    integrations:
      - name: nri-mysql
        env:
          HOSTNAME: the-mysql-host
          PORT: the-mysql-port
          USERNAME: ${nr-env:MYSQL_USER}
          PASSWORD: ${nr-env:MYSQL_PASSWORD}
          REMOTE_MONITORING: true
        interval: 10s
        labels:
          env: production
          role: write-replica
        inventory_source: config/mysql
"#;

    // config with `exec` field to be denied
    const CONFIG_WITH_EXEC: &str = r#"
################################################
# Values file for Infrastructure Agent 0.1.0
################################################

# Configuration for the Infrastructure Agent
config_agent: |
  license_key: {{ NEW_RELIC_LICENSE_KEY }}
  staging: true
  display_name: host-display-name
  enable_process_metrics: true
  log:
    level: debug
    forward: true

# Configuration for New Relic Integrations
config_integrations:
  mysql.yml: |
    integrations:
      - name: nri-mysql
        offset: 10s
        config:
          name: RandomNumbers
          exec: an extra command
  mysql.yml: |
    integrations:
      - name: nri-mysql
        env:
          HOSTNAME: the-mysql-host
          PORT: the-mysql-port
          USERNAME: ${nr-env:MYSQL_USER}
          PASSWORD: ${nr-env:MYSQL_PASSWORD}
          REMOTE_MONITORING: true
        interval: 10s
        labels:
          env: production
          role: write-replica
        inventory_source: config/mysql
"#;

    // config with `binary_path` field to be denied
    const CONFIG_WITH_BINARY_PATH_UPPERCASE: &str = r#"
################################################
# Values file for Infrastructure Agent 0.1.0
################################################

# Configuration for the Infrastructure Agent
config_agent: |
  license_key: {{ NEW_RELIC_LICENSE_KEY }}
  staging: true
  display_name: host-display-name
  enable_process_metrics: true
  log:
    level: debug
    forward: true

# Configuration for New Relic Integrations
config_integrations:
  apache.yml: |
    - name: nri-apache
      env:
        INVENTORY: "true"
        STATUS_URL: http://127.0.0.1/server-status?auto
        BINARY_PATH: "/usr/bin/whatever"
        # https://github.com/newrelic/infra-integrations-sdk/blob/master/docs/entity-definition.md
        REMOTE_MONITORING: true
      interval: 60s
      labels:
        env: production
        role: load_balancer
      inventory_source: config/apache
"#;

    // config with `binary_path` field to be denied
    const CONFIG_WITH_BINARY_PATH_LOWERCASE: &str = r#"
################################################
# Values file for Infrastructure Agent 0.1.0
################################################

# Configuration for the Infrastructure Agent
config_agent: |
  license_key: {{ NEW_RELIC_LICENSE_KEY }}
  staging: true
  display_name: host-display-name
  enable_process_metrics: true
  log:
    level: debug
    forward: true

# Configuration for New Relic Integrations
config_integrations:
  apache.yml: |
    - name: nri-apache
      env:
        INVENTORY: "true"
        STATUS_URL: http://127.0.0.1/server-status?auto
        binary_path: "/usr/bin/whatever"
        # https://github.com/newrelic/infra-integrations-sdk/blob/master/docs/entity-definition.md
        REMOTE_MONITORING: true
      interval: 60s
      labels:
        env: production
        role: load_balancer
      inventory_source: config/apache
"#;

    // infra agent config to be allowed
    const GOOD_INFRA_AGENT_CONFIG: &str = r#"
config_agent:
  license_key: your_license_key
  fedramp: true
  payload_compression_level: 7
  display_name: new_name
  passthrough_environment:
    - ONE
    - TWO
  custom_attributes:
    environment: production
    service: login service
    team: alpha-team
  enable_process_metrics: true
  include_matching_metrics:
    metric.attribute:
      - regex "pattern"
      - "string"
      - "string-with-wildcard*"
  log:
    file: /tmp/agent.log
    format: json
    level: smart
    forward: false
    stdout: false
    smart_level_entry_limit: 500
    exclude_filters:
      "*":
    include_filters:
      integration_name:
        - nri-powerdns
  network_interface_filters:
    prefix:
      - dummy
      - lo
    index-1:
      - tun
  disable_all_plugins: false
  cloud_security_group_refresh_sec: 60
  daemontools_interval_sec: 15
  dpkg_interval_sec: 30
  facter_interval_sec: 30
  kernel_modules_refresh_sec: 10
  network_interface_interval_sec: 60
  rpm_interval_sec: 30
  selinux_interval_sec: 30
  sshd_config_refresh_sec: 15
  supervisor_interval_sec: 15
  sysctl_interval_sec: 60
  systemd_interval_sec: 30
  sysvinit_interval_sec: 30
  upstart_interval_sec: 30
  users_refresh_sec: 15
  windows_services_refresh_sec: 30
  windows_updates_refresh_sec: 60
  metrics_network_sample_rate: 10
  metrics_process_sample_rate: 20
  metrics_storage_sample_rate: 20
  metrics_system_sample_rate: 5
  selinux_enable_semodule: true
  http_server_enabled: true
  http_server_host: localhost
  http_server_port: 8001
  ca_bundle_dir: /etc/my-certificates
  ca_bundle_file: /etc/my-certificates/secureproxy.pem
  ignore_system_proxy: false
  proxy: https://user:password@hostname:port
  proxy_validate_certificates: false
  max_procs: 1
  agent_dir: /some/dir
  plugin_dir: /another/dir
  entityname_integrations_v2_update: false
  pid_file: /some/pid/file
  app_data_dir: /some/app/data_dir
  cloud_max_retry_count: 10
  cloud_retry_backoff_sec: 60
  cloud_metadata_expiry_sec: 300
  disable_cloud_metadata: false
  disable_cloud_instance_id: false
  startup_connection_retries: 6
  logging_retry_limit: 5
  startup_connection_retry_time: 5s
  startup_connection_timeout: 10s
  container_cache_metadata_limit: 60
  docker_api_version: 1.24
  custom_supported_file_systems:
  file_devices_ignored:
  ignored_inventory:
  ignore_reclaimable: false
  supervisor_rpc_sock:
  proxy_config_plugin: true
  facter_home_dir:
  strip_command_line: true
  dns_hostname_resolution: true
  override_hostname: custom.hostname.org
  override_hostname_short: custom-hostname
  remove_entities_period: 48h
  enable_win_update_plugin: false
  legacy_storage_sampler: false
  win_process_priority_class: Normal
  win_removable_drives: true
  disable_zero_mem_process_filter: false
"#;

    const CONFIG_WITH_IMAGE_REPOSITORY: &str = r#"
chart_values:
  image:
    repository: some/repository
"#;

    const GOOD_K8S_NRDOT_CONFIG: &str = r#"
chart_version: "1.2.3"
chart_values:
  kube-state-metrics:
    enabled: true
    # -- Disable prometheus from auto-discovering KSM and potentially scraping duplicated data
    prometheusScrape: false

  # -------------------------------------------
  # Image is included (we can setup the tag and pullPolicy but repository is not allowed)
  # -------------------------------------------
  image:
    # -- The pull policy is defaulted to IfNotPresent, which skips pulling an image if it already exists. If pullPolicy is defined without a specific value, it is also set to Always.
    pullPolicy: IfNotPresent
    # --  Overrides the image tag whose default is the chart appVersion.
    tag: "0.7.1"

  # -- Name of the Kubernetes cluster monitored. Mandatory. Can be configured also with `global.cluster`
  cluster: ""
  # -- This set this license key to use. Can be configured also with `global.licenseKey`
  licenseKey: "xxx"
  # -- In case you don't want to have the license key in you values, this allows you to point to a user created secret to get the key from there. Can be configured also with `global.customSecretName`
  customSecretName: ""
  # -- In case you don't want to have the license key in you values, this allows you to point to which secret key is the license key located. Can be configured also with `global.customSecretLicenseKey`
  customSecretLicenseKey: ""

  # -- Additional labels for chart pods
  podLabels: {}
  # -- Additional labels for chart objects
  labels: {}

  # -- Sets pod's priorityClassName. Can be configured also with `global.priorityClassName`
  priorityClassName: ""

  # -- Sets pod's dnsConfig. Can be configured also with `global.dnsConfig`
  dnsConfig: {}

  # -- Run the integration with full access to the host filesystem and network.
  # Running in this mode allows reporting fine-grained cpu, memory, process and network metrics for your nodes.
  # @default -- `true`
  privileged: true

  daemonset:
    # -- Sets daemonset pod node selector. Overrides `nodeSelector` and `global.nodeSelector`
    nodeSelector: {}
    # -- Sets daemonset pod tolerations. Overrides `tolerations` and `global.tolerations`
    tolerations: []
    # -- Sets daemonset pod affinities. Overrides `affinity` and `global.affinity`
    affinity: {}
    # -- Annotations to be added to the daemonset.
    podAnnotations: {}
    # -- Sets security context (at pod level) for the daemonset. Overrides `podSecurityContext` and `global.podSecurityContext`
    podSecurityContext: {}
    # -- Sets security context (at container level) for the daemonset. Overrides `containerSecurityContext` and `global.containerSecurityContext`
    containerSecurityContext:
      privileged: true
    # -- Sets resources for the daemonset.
    resources: {}
    # -- Settings for daemonset configmap
    # @default -- See `values.yaml`
    configMap:
      # -- OpenTelemetry config for the daemonset. If set, overrides default config and disables configuration parameters for the daemonset.
      config: {}

  deployment:
    # -- Sets deployment pod node selector. Overrides `nodeSelector` and `global.nodeSelector`
    nodeSelector: {}
    # -- Sets deployment pod tolerations. Overrides `tolerations` and `global.tolerations`
    tolerations: []
    # -- Sets deployment pod affinities. Overrides `affinity` and `global.affinity`
    affinity: {}
    # -- Annotations to be added to the deployment.
    podAnnotations: {}
    # -- Sets security context (at pod level) for the deployment. Overrides `podSecurityContext` and `global.podSecurityContext`
    podSecurityContext: {}
    # -- Sets security context (at container level) for the deployment. Overrides `containerSecurityContext` and `global.containerSecurityContext`
    containerSecurityContext: {}
    # -- Sets resources for the deployment.
    resources: {}
    # -- Settings for deployment configmap
    # @default -- See `values.yaml`
    configMap:
      # -- OpenTelemetry config for the deployment. If set, overrides default config and disables configuration parameters for the deployment.
      config: {}

  # -- Sets all pods' node selector. Can be configured also with `global.nodeSelector`
  nodeSelector: {}
  # -- Sets all pods' tolerations to node taints. Can be configured also with `global.tolerations`
  tolerations: []
  # -- Sets all pods' affinities. Can be configured also with `global.affinity`
  affinity: {}
  # -- Sets all security contexts (at pod level). Can be configured also with `global.securityContext.pod`
  podSecurityContext: {}
  # -- Sets all security context (at container level). Can be configured also with `global.securityContext.container`
  containerSecurityContext: {}

  rbac:
    # -- Specifies whether RBAC resources should be created
    create: true

  # -- Settings controlling ServiceAccount creation
  # @default -- See `values.yaml`
  serviceAccount:
    # serviceAccount.create -- (bool) Specifies whether a ServiceAccount should be created
    # @default -- `true`
    create:
    # If not set and create is true, a name is generated using the fullname template
    name: ""
    # Specify any annotations to add to the ServiceAccount
    annotations:

  # -- (bool) Sets the debug logs to this integration or all integrations if it is set globally. Can be configured also with `global.verboseLog`
  # @default -- `false`
  verboseLog:

  # -- (bool) Send the metrics to the staging backend. Requires a valid staging license key. Can be configured also with `global.nrStaging`
  # @default -- `false`
  nrStaging:

  receivers:
    prometheus:
      # -- (bool) Specifies whether the `prometheus` receiver is enabled
      # @default -- `true`
      enabled: true
      # -- Sets the scrape interval for the `prometheus` receiver
      # @default -- `1m`
      scrapeInterval: 1m
    k8sEvents:
      # -- (bool) Specifies whether the `k8s_events` receiver is enabled
      # @default -- `true`
      enabled: true
    hostmetrics:
      # -- (bool) Specifies whether the `hostmetrics` receiver is enabled
      # @default -- `true`
      enabled: true
      # -- Sets the scrape interval for the `hostmetrics` receiver
      # @default -- `1m`
      scrapeInterval: 1m
    kubeletstats:
      # -- (bool) Specifies whether the `kubeletstats` receiver is enabled
      # @default -- `true`
      enabled: true
      # -- Sets the scrape interval for the `kubeletstats` receiver
      # @default -- `1m`
      scrapeInterval: 1m
    filelog:
      # -- (bool) Specifies whether the `filelog` receiver is enabled
      # @default -- `true`
      enabled: true

  # -- (bool) Send only the [metrics required](https://github.com/newrelic/helm-charts/tree/master/charts/nr-k8s-otel-collector/docs/metrics-lowDataMode.md) to light up the NR kubernetes UI, this agent defaults to setting lowDataMode true, but if this setting is unset, lowDataMode will be set to false
  # @default -- `false`
  lowDataMode: true
"#;
}
