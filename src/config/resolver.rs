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
                    AgentType::InfraAgent(Some("otherinstance".to_owned())),
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
    fn resolve_agents_with_custom_configs() {}

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
