use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat, Source};

use super::{agent_configs::MetaAgentConfig, error::MetaAgentConfigError};

/// Builder for the configuration, managing if it is loaded from a default expected file or from
/// a custom one provided by the command line arguments.
pub struct Resolver(ConfigBuilder<DefaultState>);

impl Resolver {
    fn new<T>(source: T) -> Self
    where
        T: Source + Send + Sync + 'static,
    {
        let builder = Config::builder().add_source(source);
        Self(builder)
    }

    fn with_file_source(self, file: &Path) -> Self {
        Self(
            self.0
                .add_source(File::new(file.to_string_lossy().as_ref(), FileFormat::Yaml)),
        )
    }

    fn build_config(self) -> Result<MetaAgentConfig, MetaAgentConfigError> {
        Ok(self.0.build()?.try_deserialize::<MetaAgentConfig>()?)
    }

    /// Attempts to build the configuration
    pub fn retrieve_config(file: &Path) -> Result<MetaAgentConfig, MetaAgentConfigError> {
        Self::default().with_file_source(file).build_config()
    }
}

impl Default for Resolver {
    fn default() -> Self {
        let builder = Config::builder();
        Self(builder)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::config::{
        agent_configs::MetaAgentConfig, agent_type::AgentType, resolver::Resolver,
    };
    use config::{Value, ValueKind};

    #[test]
    fn resolve_one_agent() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = MetaAgentConfig {
            agents: [(
                AgentType::InfraAgent(None),
                Value::new(None, ValueKind::Nil),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_two_different_agents() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
# both enabled
agents:
  nr_infra_agent:
  nr_otel_collector:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = MetaAgentConfig {
            agents: [
                (
                    AgentType::InfraAgent(None),
                    Value::new(None, ValueKind::Nil),
                ),
                (AgentType::Nrdot(None), Value::new(None, ValueKind::Nil)),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_same_type_agents() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
# both enabled
agents:
  nr_infra_agent:
  nr_otel_collector:
  nr_infra_agent/otherinstance:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = MetaAgentConfig {
            agents: [
                (
                    AgentType::InfraAgent(None),
                    Value::new(None, ValueKind::Nil),
                ),
                (
                    AgentType::InfraAgent(Some("otherinstance".to_string())),
                    Value::new(None, ValueKind::Nil),
                ),
                (AgentType::Nrdot(None), Value::new(None, ValueKind::Nil)),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_config_with_unexpected_fields() {
        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
this_is_another_random_config: value
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();
        let expected = MetaAgentConfig {
            agents: [(
                AgentType::InfraAgent(None),
                Value::new(None, ValueKind::Nil),
            )]
            .iter()
            .cloned()
            .collect(),
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_empty_agents_field() {
        assert!(Resolver::new(File::from_str(
            "
agents:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .is_err());
    }

    #[test]
    fn resolve_agents_with_custom_configs() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
agents:
  nr_infra_agent:
    configValue: value
    configList: [value1, value2]
    configMap:
      key1: value1
      key2: value2
  nr_otel_collector:
  nr_infra_agent/otherinstance:
    otherConfigValue: value
    otherConfigList: [value1, value2]
    otherConfigMap:
      key1: value1
      key2: value2
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        // Deserializing with the serde_yaml crate because putting
        // the literal Value representations here is too verbose!
        let expected_nria_conf = serde_yaml::from_str::<Value>(
            r#"
            configValue: value
            configList: [value1, value2]
            configMap:
                key1: value1
                key2: value2
            "#,
        )
        .unwrap();
        let expected_otherinstance_nria_conf = serde_yaml::from_str::<Value>(
            r#"
            otherConfigValue: value
            otherConfigList: [value1, value2]
            otherConfigMap:
                key1: value1
                key2: value2
            "#,
        )
        .unwrap();

        let expected = MetaAgentConfig {
            agents: [
                (AgentType::InfraAgent(None), expected_nria_conf),
                (
                    AgentType::InfraAgent(Some("otherinstance".to_string())),
                    expected_otherinstance_nria_conf,
                ),
                (AgentType::Nrdot(None), Value::new(None, ValueKind::Nil)),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_custom_agent_with_invalid_config() {
        let actual = Resolver::new(File::from_str(
            r#"
agents:
  custom_agent:
    bin: echo
  custom_agent/nobin:
    args:
      - "hello"
      - "world"
  custom_agent/binargs:
    bin: echo
    args:
      - "hello"
      - "world"
"#,
            FileFormat::Yaml,
        ))
        .build_config();
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("custom agent type `custom_agent/nobin` must have a `bin` key"));
    }
}
