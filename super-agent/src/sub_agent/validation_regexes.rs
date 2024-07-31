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

// deny using custom images for nr-dot
// https://github.com/newrelic/helm-charts/blob/nr-k8s-otel-collector-0.7.4/charts/nr-k8s-otel-collector/values.yaml#L16
// Example:
// chart_values:
//   image:
//     repository: newrelic/nr-otel-collector
//     pullPolicy: IfNotPresent
//     tag: "0.7.1"
pub static REGEX_IMAGE_REPOSITORY: &str = "repository\\s*:";

#[cfg(test)]
mod test {
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::test::VALID_ONHOST_NRDOT_CONFIG;
    use crate::sub_agent::config_validator::{
        ConfigValidator, ValidatorError, FQN_NAME_INFRA_AGENT, FQN_NAME_NRDOT,
    };
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    #[test]
    fn test_valid_configs_are_allowed() {
        let config_validator = ConfigValidator::try_new().unwrap();

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
            agent_type: AgentTypeFQN,
            config: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let config_validator = ConfigValidator::try_new().unwrap();
                let remote_config = remote_config(self.config);
                let err = config_validator
                    .validate(&self.agent_type, &remote_config)
                    .unwrap_err();

                assert_matches!(err, ValidatorError::InvalidConfig);
            }
        }
        let test_cases = vec![
            TestCase {
                _name: "infra-agent config with nri-flex should be invalid",
                agent_type: infra_agent(),
                config: CONFIG_WITH_NRI_FLEX,
            },
            TestCase {
                _name: "infra-agent config with command should be invalid",
                agent_type: infra_agent(),
                config: CONFIG_WITH_COMMAND,
            },
            TestCase {
                _name: "infra-agent config with exec should be invalid",
                agent_type: infra_agent(),
                config: CONFIG_WITH_EXEC,
            },
            TestCase {
                _name: "infra-agent config with binary_path uppercase should be invalid",
                agent_type: infra_agent(),
                config: CONFIG_WITH_BINARY_PATH_UPPERCASE,
            },
            TestCase {
                _name: "infra-agent config with binary_path lowercase should be invalid",
                agent_type: infra_agent(),
                config: CONFIG_WITH_BINARY_PATH_LOWERCASE,
            },
            TestCase {
                _name: "nrdot config with image repository  should be invalid",
                agent_type: nrdot(),
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

    fn infra_agent() -> AgentTypeFQN {
        AgentTypeFQN::try_from(format!("newrelic/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str()).unwrap()
    }

    fn nrdot() -> AgentTypeFQN {
        AgentTypeFQN::try_from(format!("newrelic/{}:0.0.1", FQN_NAME_NRDOT).as_str()).unwrap()
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
