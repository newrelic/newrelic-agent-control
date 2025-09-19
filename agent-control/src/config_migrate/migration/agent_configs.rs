use std::{iter, path::PathBuf};

use fs::file_reader::{FileReader, FileReaderError};
use serde::Deserialize;
use serde_yaml::Sequence;
use thiserror::Error;
use tracing::debug;

use crate::config_migrate::migration::config::DirInfo;

/// Configuration for the infrastructure agent, represented as a raw YAML value.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
#[serde(transparent)]
pub struct NewRelicInfraConfig(serde_yaml::Value);

/// Configuration for the integrations. The `integrations` key is required.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
pub struct IntegrationsConfig {
    // Integrations must be a list of config items.
    #[serde(default)]
    integrations: Vec<ConfigItem>,
    // We don't perform any validations on the `discovery` key, so we represent it with `Value`.
    #[serde(default)]
    discovery: serde_yaml::Value,
    // We don't perform any validations on the `variables` key, so we represent it with `Value`.
    #[serde(default)]
    variables: serde_yaml::Value,
}

/// Configuration for the log forwarder, represented as a list of raw YAML mappings.
#[derive(Debug, Default, PartialEq, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default)]
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

/// Wrapper around a supported configuration value that can be merged with other values of the same key.
/// The inner value is a tuple where the first element is the key (e.g., "logs", "integrations") and the second element is a sequence of values.
/// This structure allows merging multiple configuration entries under the same key by concatenating their sequences.
///
/// This is intended to model the configuration files for integrations and logs compatible with the New Relic Infrastructure Agent. For example:
///
/// ```yaml
/// integrations:
///   - name: nri-docker
///     when:
///       feature: docker_enabled
///       file_exists: /var/run/docker.sock
///     interval: 15s
///   - name: nri-docker
///     when:
///       feature: docker_enabled
///       env_exists:
///         FARGATE: "true"
///     interval: 15s
/// ```
#[derive(Debug, Default, PartialEq, Clone)]
pub struct SupportedConfigValue((String, Sequence));

#[derive(Debug, Error)]
pub enum SupportedConfigValueError {
    #[error("error reading file `{0}`: `{1}`")]
    ReadFileError(PathBuf, FileReaderError),

    #[error("error parsing supported config value from file `{0}`: `{1}`")]
    ParseError(PathBuf, serde_yaml::Error),
}

impl<'de> Deserialize<'de> for SupportedConfigValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct SupportedConfigValueVisitor;

        impl<'de> Visitor<'de> for SupportedConfigValueVisitor {
            type Value = SupportedConfigValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with a single key and a sequence of values")
            }

            fn visit_map<M>(self, mut map: M) -> Result<SupportedConfigValue, M::Error>
            where
                M: MapAccess<'de>,
            {
                if let Some((key, value)) = map.next_entry::<String, Sequence>()? {
                    if map.next_key::<String>()?.is_some() {
                        Err(M::Error::custom(
                            "expected a single key-value pair in the map",
                        ))
                    } else {
                        Ok(SupportedConfigValue((key, value)))
                    }
                } else {
                    Err(M::Error::custom("expected a non-empty map"))
                }
            }
        }

        deserializer.deserialize_map(SupportedConfigValueVisitor)
    }
}

impl SupportedConfigValue {
    /// Attempts to merge another `SupportedConfigValue` into this one.
    /// Merging is only possible if both values have the same key (the first element of the tuple).
    /// The key of [`self`] is used as the key of the resulting merged value.
    /// If the keys match, the sequences (the second element of the tuple) are concatenated.
    /// If the keys do not match, we return the [`self`] value unchanged.
    pub fn merge(mut self, other: Self) -> Self {
        if self.0.0 != other.0.0 {
            debug!(
                "cannot merge incompatible config values. Left value key: `{}`, right value key: `{}`",
                self.0.0, other.0.0
            );
        } else {
            self.0.1.extend(other.0.1);
        }
        self
    }

    fn default_with_key(key: impl AsRef<str>) -> Self {
        SupportedConfigValue((key.as_ref().to_string(), Sequence::new()))
    }

    pub fn from_dir_with_key(
        file_reader: &impl FileReader,
        dir_info: &DirInfo,
        key: impl AsRef<str>,
    ) -> Result<Self, SupportedConfigValueError> {
        file_reader
            .dir_entries(&dir_info.path)
            .unwrap_or_default()
            .iter()
            .filter(|p| dir_info.valid_filename(p))
            .map(|p| {
                let file = file_reader
                    .read(p)
                    .map_err(|e| SupportedConfigValueError::ReadFileError(p.to_path_buf(), e))?;
                serde_yaml::from_str::<SupportedConfigValue>(&file)
                    .map_err(|e| SupportedConfigValueError::ParseError(p.to_path_buf(), e))
            })
            .try_fold(SupportedConfigValue::default_with_key(key), |acc, item| {
                item.map(|v| acc.merge(v))
            })
    }
}

impl From<SupportedConfigValue> for serde_yaml::Value {
    fn from(value: SupportedConfigValue) -> Self {
        use serde_yaml::Mapping;
        use serde_yaml::Value::*;

        let k = String(value.0.0);
        let v = Sequence(value.0.1);
        Mapping(Mapping::from_iter(iter::once((k, v))))
    }
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

    #[test]
    fn test_parse_and_merge_integration_config() {
        let config1: SupportedConfigValue =
            serde_yaml::from_str(EXAMPLE_INTEGRATION_CONFIG).unwrap();
        let config2: SupportedConfigValue =
            serde_yaml::from_str(EXAMPLE_INTEGRATION_CONFIG).unwrap();
        let merged_config = config1.merge(config2);
        assert_eq!(merged_config.0.1.len(), 4);
    }

    #[test]
    fn test_parse_and_merge_logs_config() {
        let config1: SupportedConfigValue = serde_yaml::from_str(EXAMPLE_LOGS_CONFIG).unwrap();
        let config2: SupportedConfigValue = serde_yaml::from_str(EXAMPLE_LOGS_CONFIG).unwrap();
        let merged_config = config1.merge(config2);
        assert_eq!(merged_config.0.1.len(), 12);
    }

    #[test]
    fn test_uncompatible_merge() {
        let config1: SupportedConfigValue = serde_yaml::from_str(EXAMPLE_LOGS_CONFIG).unwrap();
        let config2: SupportedConfigValue =
            serde_yaml::from_str(EXAMPLE_INTEGRATION_CONFIG).unwrap();
        let result = config1.clone().merge(config2);
        assert_eq!(result, config1);
    }

    #[test]
    fn bad_serde() {
        let result: Result<SupportedConfigValue, _> = serde_yaml::from_str(r#"{}"#);
        assert!(result.is_err_and(|e| e.to_string().contains("expected a non-empty map")));

        let result: Result<SupportedConfigValue, _> = serde_yaml::from_str(r#"{"key": "value"}"#);
        assert!(result.is_err_and(|e| { e.to_string().contains("expected a sequence") }));

        let result: Result<SupportedConfigValue, _> = serde_yaml::from_str(r#"{"k1":[],"k2":[]}"#);
        assert!(result.is_err_and(|e| e.to_string().contains("expected a single key-value pair")));
    }
}
