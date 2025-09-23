use serde::Deserialize;

/// Configuration for the infrastructure agent, represented as a raw YAML value.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
#[serde(transparent)]
pub struct NewRelicInfraConfig(serde_yaml::Value);

/// Configuration for the integrations. The `integrations` key is required.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
pub struct IntegrationsConfig {
    // Integrations must be a list of config items.
    integrations: Vec<ConfigItem>,
    // We don't perform any validations on the `discovery` key, so we represent it with `Value`.
    discovery: Option<serde_yaml::Value>,
    // We don't perform any validations on the `variables` key, so we represent it with `Value`.
    variables: Option<serde_yaml::Value>,
}

/// Configuration for the log forwarder, represented as a list of raw YAML mappings.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
pub struct LoggingConfig {
    logs: Vec<ConfigItem>,
}

/// Arbitrary configuration item used in the integrations and logging configs.
///
/// The `name` key is required. All other keys are captured in `extra_attrs`.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
struct ConfigItem {
    name: String,
    #[serde(flatten)]
    extra_attrs: serde_yaml::Mapping,
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const EXAMPLE_INTEGRATION_CONFIG: &str = r#"
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

discovery:
  arbitrary_key: arbitrary_value

variables:
  arbitrary_key: arbitrary_value
"#;

    pub const EXAMPLE_LOGS_CONFIG: &str = r#"
###############################################################################
# Log forwarder configuration file example                                    #
# Source: file                                                                #
# Available customization parameters: attributes, max_line_kb, pattern        #
###############################################################################
logs:
  # Basic tailing of a single file
  - name: basic-file
    file: /var/log/logFile.log

  # File with spaces in its path. No need to use quotes.
  - name: file-with-spaces-in-path
    file: /var/log/folder with spaces/logFile.log

  # Specify a list of custom attributes, as key-value pairs, to be included
  # in each log record
  - name: file-with-attributes
    file: /var/log/logFile.log
    attributes:
      application: tomcat
      department: sales
      maintainer: example@mailprovider.com

  # Use wildcards to refer to multiple files having a common extension or
  # prefix. Newly generated files will be automatically detected every 60
  # seconds.
  #
  # WARNING: avoid using wildcards that include the file extension, since
  # it'll cause logs to be forwarded repeatedly if log rotation is enabled.
  - name: log-files-in-folder
    file: /var/log/logF*.log

  # Lines longer than 128 KB will be automatically skipped. Use 'max_line_kb'
  # to increase this limit.
  - name: log-file-with-long-lines
    file: /var/log/logFile.log
    max_line_kb: 256

  # Use 'pattern' to filter records using a regular expression
  - name: only-records-with-warn-and-error
    file: /var/log/logFile.log
    pattern: WARN|ERROR
"#;

    // Not testing the parsing of infra agent config, as we
    // represent it as an arbitrary YAML value here for simplicity.

    #[test]
    fn serde_logs() {
        let config: LoggingConfig = serde_yaml::from_str(EXAMPLE_LOGS_CONFIG).unwrap();
        assert_eq!(config.logs.len(), 6);
        assert_eq!(config.logs[0].name, "basic-file");
        assert_eq!(config.logs[1].name, "file-with-spaces-in-path");
        assert_eq!(config.logs[2].name, "file-with-attributes");
        assert_eq!(config.logs[3].name, "log-files-in-folder");
        assert_eq!(config.logs[4].name, "log-file-with-long-lines");
        assert_eq!(config.logs[5].name, "only-records-with-warn-and-error");

        // no logs key should fail
        let err = serde_yaml::from_str::<LoggingConfig>("").unwrap_err();
        assert!(err.to_string().contains("missing field `logs`"));
    }

    #[test]
    fn serde_integrations() {
        let config: IntegrationsConfig = serde_yaml::from_str(EXAMPLE_INTEGRATION_CONFIG).unwrap();
        assert_eq!(config.integrations.len(), 2);
        assert_eq!(config.integrations[0].name, "nri-docker");
        assert_eq!(config.integrations[1].name, "nri-docker");

        // only integrations key (though empty) should succeed:
        let config: IntegrationsConfig = serde_yaml::from_str("integrations: []").unwrap();
        assert_eq!(config.integrations.len(), 0);

        // no integrations key should fail
        let err = serde_yaml::from_str::<IntegrationsConfig>("").unwrap_err();
        assert!(err.to_string().contains("missing field `integrations`"));
    }
}
