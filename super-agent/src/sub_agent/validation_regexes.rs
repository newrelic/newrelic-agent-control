// otel endpoint regex.
pub static REGEX_OTEL_ENDPOINT: &str = r"\s*endpoint\s*:\s*(.+)";
pub static REGEX_VALID_OTEL_ENDPOINT: &str = r#"^"?(https://)?(staging-otlp\.nr-data\.net|otlp\.nr-data\.net|otlp\.eu01\.nr-data\.net|\$\{OTEL_EXPORTER_OTLP_ENDPOINT\})(:\d+)?"?$"#;

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
mod test {
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::{
        ConfigValidator, ValidatorError, FQN_NAME_INFRA_AGENT,
    };
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    #[test]
    fn test_valid_configs_are_allowed() {
        let config_validator = ConfigValidator::try_new().unwrap();
        let remote_config = remote_config(GOOD_INFRA_AGENT_CONFIG);
        let result = config_validator.validate(&infra_agent(), &remote_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_configs_are_blocked() {
        struct TestCase {
            _name: &'static str,
            config: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let config_validator = ConfigValidator::try_new().unwrap();
                let remote_config = remote_config(self.config);
                let err = config_validator
                    .validate(&infra_agent(), &remote_config)
                    .unwrap_err();

                assert_matches!(err, ValidatorError::InvalidConfig);
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "config with nri-flex should be invalid",
                config: CONFIG_WITH_NRI_FLEX,
            },
            TestCase {
                _name: "config with command should be invalid",
                config: CONFIG_WITH_COMMAND,
            },
            TestCase {
                _name: "config with exec should be invalid",
                config: CONFIG_WITH_EXEC,
            },
            TestCase {
                _name: "config with binary_path uppercase should be invalid",
                config: CONFIG_WITH_BINARY_PATH_UPPERCASE,
            },
            TestCase {
                _name: "config with binary_path lowercase should be invalid",
                config: CONFIG_WITH_BINARY_PATH_LOWERCASE,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    ///////////////////////////////////////////////////////
    // Helpers
    ///////////////////////////////////////////////////////

    fn infra_agent() -> AgentTypeFQN {
        AgentTypeFQN::try_from(format!("newrelic/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str()).unwrap()
    }

    fn remote_config(config: &str) -> RemoteConfig {
        RemoteConfig::new(
            AgentID::new("invented").unwrap(),
            Hash::new("this-is-a-hash".to_string()),
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
}
