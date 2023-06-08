use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::MetaAgentConfig, error::MetaAgentConfigError};

const DEFAULT_STATIC_CONFIG: &str = "/tmp/static.yaml";

pub struct Resolver(ConfigBuilder<DefaultState>);

impl Resolver {
    pub fn new(file: &Path) -> Self {
        let builder = Config::builder()
            .add_source(File::new(file.to_string_lossy().as_ref(), FileFormat::Yaml));
        Self(builder)
    }
}

impl Default for Resolver {
    fn default() -> Self {
        let builder =
            Config::builder().add_source(File::new(DEFAULT_STATIC_CONFIG, FileFormat::Yaml));

        Self(builder)
    }
}

impl Resolver {
    pub fn build_config(self) -> Result<MetaAgentConfig, MetaAgentConfigError> {
        Ok(self.0.build()?.try_deserialize::<MetaAgentConfig>()?)
    }
}

#[cfg(test)]
mod tests {

    use std::path::Path;

    use crate::config::{agent_configs::MetaAgentConfig, resolver::Resolver};

    use config::{Value, ValueKind};

    use crate::config::agent_type::AgentType;

    #[test]
    fn resolve_one_agent() {
        // Build the config
        let actual = Resolver::new(Path::new("tests/config/assets/one_agent.yml"))
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
        let actual = Resolver::new(Path::new("tests/config/assets/two_agents.yml"))
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
        let actual = Resolver::new(Path::new("tests/config/assets/repeated_types.yml"))
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
    fn resolve_agents_with_custom_configs() {
        // Build the config
        let actual = Resolver::new(Path::new("tests/config/assets/with_custom_configs.yml"))
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
    fn resolve_config_with_unexpected_fields() {
        let actual = Resolver::new(Path::new("tests/config/assets/non_agent_configs.yml"))
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
        let actual =
            Resolver::new(Path::new("tests/config/assets/empty_agents.yml")).build_config();
        assert!(actual.is_err());
    }

    #[test]
    fn resolve_custom_agent_with_invalid_config() {
        let actual =
            Resolver::new(Path::new("tests/config/assets/custom_agent_no_bin.yml")).build_config();
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("custom agent type `custom_agent/nobin` must have a `bin` key"));
    }
}
