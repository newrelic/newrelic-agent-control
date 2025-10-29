use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::region::Region;
use std::collections::HashMap;

const INFRA_AGENT_TYPE_FIELD: &str = "config_agent";
pub const INFRA_AGENT_TYPE_VERSION: &str = "newrelic/com.newrelic.infrastructure:0.1.0";

/// Represents the values to create or migrate an infra-config
pub struct InfraConfig {
    values: HashMap<String, serde_yaml::Value>,
    deletions: Vec<serde_yaml::Value>,
}

impl Default for InfraConfig {
    fn default() -> InfraConfig {
        Self {
            values: HashMap::from([
                (
                    "license_key".to_string(),
                    serde_yaml::Value::String("{{NEW_RELIC_LICENSE_KEY}}".to_string()),
                ),
                (
                    "enable_process_metrics".to_string(),
                    serde_yaml::Value::Bool(true),
                ),
                (
                    "status_server_enabled".to_string(),
                    serde_yaml::Value::Bool(true),
                ),
                (
                    "status_server_port".to_string(),
                    serde_yaml::Value::Number(18003.into()),
                ),
            ]),
            deletions: vec![
                serde_yaml::Value::String("staging".to_string()),
                serde_yaml::Value::String("enable_process_metrics".to_string()),
                serde_yaml::Value::String("status_server_enabled".to_string()),
                serde_yaml::Value::String("status_server_port".to_string()),
                serde_yaml::Value::String("license_key".to_string()),
                serde_yaml::Value::String("custom_attributes".to_string()),
                serde_yaml::Value::String("is_integrations_only".to_string()),
            ],
        }
    }
}

impl InfraConfig {
    #[allow(dead_code)]
    fn new(
        values: HashMap<String, serde_yaml::Value>,
        deletions: Vec<serde_yaml::Value>,
    ) -> InfraConfig {
        InfraConfig { values, deletions }
    }

    pub fn with_custom_attributes(mut self, custom_attributes: &str) -> Result<Self, CliError> {
        if !custom_attributes.trim().is_empty() {
            let custom_attributes_value: serde_yaml::Value =
                serde_yaml::from_str(custom_attributes).map_err(|err| {
                    CliError::Command(format!("error parsing custom attributes: {err}"))
                })?;
            if let Some(attributes) = custom_attributes_value.as_mapping() {
                for (key, value) in attributes {
                    key.as_str()
                        .and_then(|key| self.values.insert(key.to_string(), value.clone()));
                }
            }
        }
        Ok(self)
    }

    pub fn with_region(mut self, region: Region) -> Self {
        if region == Region::STAGING {
            self.values
                .insert("staging".to_string(), serde_yaml::Value::Bool(true));
        }
        self
    }

    pub fn with_proxy(mut self, proxy: &str) -> Self {
        if !proxy.trim().is_empty() {
            self.deletions
                .push(serde_yaml::Value::String("proxy".to_string()));
            self.values.insert(
                "proxy".to_string(),
                serde_yaml::Value::String(proxy.to_string()),
            );
        }
        self
    }

    pub fn values(&self) -> &HashMap<String, serde_yaml::Value> {
        &self.values
    }

    pub fn generate_agent_type_config_mapping(
        self,
        config_mapping: &str,
    ) -> Result<String, CliError> {
        let mut parsed_yaml: serde_yaml::Value =
            serde_yaml::from_str(config_mapping).map_err(|err| {
                CliError::Command(format!("error parsing agent type config mapping: {err}"))
            })?;

        if let Some(config_agent) = parsed_yaml
            .get_mut("configs")
            .and_then(|configs| configs.as_sequence_mut())
            .and_then(|configs| configs.get_mut(0))
            .and_then(|config| config.get_mut("filesystem_mappings"))
            .and_then(|mappings| mappings.get_mut("config_agent"))
            .and_then(|agent| agent.as_mapping_mut())
        {
            config_agent.insert(
                serde_yaml::Value::String("overwrites".to_string()),
                serde_yaml::Value::Mapping(
                    self.values
                        .into_iter()
                        .map(|(k, v)| (serde_yaml::Value::String(k.to_string()), v))
                        .collect(),
                ),
            );
            config_agent.insert(
                serde_yaml::Value::String("deletions".to_string()),
                serde_yaml::Value::Sequence(self.deletions),
            );
        }

        serde_yaml::to_string(&parsed_yaml).map_err(|err| {
            CliError::Command(format!("error generating agent type config mapping: {err}"))
        })
    }

    pub fn generate_infra_config_values(&self) -> Result<String, CliError> {
        let mut config_agent = serde_yaml::Mapping::new();

        for (key, value) in &self.values {
            config_agent.insert(serde_yaml::Value::String(key.clone()), value.clone());
        }

        let mut root_mapping = serde_yaml::Mapping::new();
        root_mapping.insert(
            serde_yaml::Value::String(INFRA_AGENT_TYPE_FIELD.to_string()),
            serde_yaml::Value::Mapping(config_agent),
        );

        serde_yaml::to_string(&serde_yaml::Value::Mapping(root_mapping))
            .map_err(|err| CliError::Command(format!("error generating infra config: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::on_host::config_gen::region::Region;
    use crate::config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
    use serde_yaml::Value;

    const EXPECTED_AGENT_TYPE_CONFIG: &str = r#"configs:
- agent_type_fqn: newrelic/com.newrelic.infrastructure:0.1.0
  filesystem_mappings:
    config_agent:
      file_path: /etc/newrelic-infra.yml
      overwrites:
        custom_attributes:
          test: '123'
      deletions:
      - staging
      - enable_process_metrics
      - status_server_enabled
      - status_server_port
      - license_key
      - custom_attributes
      - is_integrations_only
    config_integrations:
      dir_path: /etc/newrelic-infra/integrations.d
      extensions:
      - yml
      - yaml
    config_logging:
      dir_path: /etc/newrelic-infra/logging.d
      extensions:
      - yml
      - yaml
"#;

    #[test]
    fn test_generate_agent_type_config_mapping() {
        let mut values = HashMap::new();
        // Create a nested custom_attributes value
        let mut custom_attributes = serde_yaml::Mapping::new();
        custom_attributes.insert(
            Value::String("test".to_string()),
            Value::String("123".to_string()),
        );
        values.insert(
            "custom_attributes".to_string(),
            Value::Mapping(custom_attributes),
        );

        let deletions = vec![
            Value::String("staging".to_string()),
            Value::String("enable_process_metrics".to_string()),
            Value::String("status_server_enabled".to_string()),
            Value::String("status_server_port".to_string()),
            Value::String("license_key".to_string()),
            Value::String("custom_attributes".to_string()),
            Value::String("is_integrations_only".to_string()),
        ];

        let infra_config = InfraConfig::new(values, deletions);
        let result = infra_config
            .generate_agent_type_config_mapping(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING)
            .unwrap();

        assert_eq!(result, EXPECTED_AGENT_TYPE_CONFIG);
    }

    #[test]
    fn test_generate_infra_config_values() {
        let custom_attributes = r#"custom_attributes:
  custom_key: custom_value
"#;
        let infra_config = InfraConfig::default()
            .with_custom_attributes(custom_attributes)
            .unwrap()
            .with_region(Region::STAGING)
            .with_proxy("http://proxy.example.com");
        let result = infra_config.generate_infra_config_values().unwrap();

        // Parse the YAML content
        let parsed_values: serde_yaml::Value = serde_yaml::from_str(&result).unwrap();

        if let serde_yaml::Value::Mapping(map) = parsed_values {
            if let Some(serde_yaml::Value::Mapping(config_agent_map)) =
                map.get(serde_yaml::Value::String("config_agent".to_string()))
            {
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "status_server_enabled".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String(
                        "enable_process_metrics".to_string()
                    )),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("license_key".to_string())),
                    Some(&serde_yaml::Value::String(
                        "{{NEW_RELIC_LICENSE_KEY}}".to_string()
                    ))
                );
                assert_eq!(
                    config_agent_map
                        .get(serde_yaml::Value::String("status_server_port".to_string())),
                    Some(&serde_yaml::Value::Number(serde_yaml::Number::from(18003)))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("staging".to_string())),
                    Some(&serde_yaml::Value::Bool(true))
                );
                assert_eq!(
                    config_agent_map.get(serde_yaml::Value::String("proxy".to_string())),
                    Some(&serde_yaml::Value::String(
                        "http://proxy.example.com".to_string()
                    ))
                );
                let mut custom_attributes = serde_yaml::Mapping::new();
                custom_attributes.insert(
                    serde_yaml::Value::String("custom_key".to_string()),
                    serde_yaml::Value::String("custom_value".to_string()),
                );
                assert_eq!(
                    config_agent_map
                        .get(serde_yaml::Value::String("custom_attributes".to_string())),
                    Some(&serde_yaml::Value::Mapping(custom_attributes))
                );
            }
        } else {
            panic!("Expected a YAML mapping");
        }
    }
}
