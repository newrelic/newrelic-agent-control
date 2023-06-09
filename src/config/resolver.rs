use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::MetaAgentConfig, error::MetaAgentConfigError, ConfigResolver};

const DEFAULT_STATIC_CONFIG: &str = "/tmp/static.yaml";

/// Builder for the configuration, managing if it is loaded from a default expected file or from
/// a custom one provided by the command line arguments.
pub struct Resolver(ConfigBuilder<DefaultState>);

impl Resolver {
    fn new(file: &Path) -> Self {
        let builder = Config::builder()
            .add_source(File::new(file.to_string_lossy().as_ref(), FileFormat::Yaml));
        Self(builder)
    }

    fn build_config(self) -> Result<MetaAgentConfig, MetaAgentConfigError> {
        Ok(self.0.build()?.try_deserialize::<MetaAgentConfig>()?)
    }

    // /// Attempts to build the configuration
    // pub fn retrieve_config(file: Option<&Path>) -> Result<MetaAgentConfig, MetaAgentConfigError> {
    //     match file {
    //         Some(f) => Self::new(f).build_config(),
    //         None => Self::default().build_config(),
    //     }
    // }
}

impl Default for Resolver {
    fn default() -> Self {
        let builder =
            Config::builder().add_source(File::new(DEFAULT_STATIC_CONFIG, FileFormat::Yaml));
        Self(builder)
    }
}

impl ConfigResolver for Option<&Path> {
    type Output = MetaAgentConfig;
    type Error = MetaAgentConfigError;

    fn resolve(self) -> Result<Self::Output, Self::Error> {
        match self {
            Some(f) => Resolver::new(f).build_config(),
            None => Resolver::default().build_config(),
        }
    }
}

#[cfg(test)]
mod tests {

    use config::{Value, ValueKind};

    use crate::config::agent_type::AgentType;

    use super::*;

    type MockedConfig = &'static str;

    impl ConfigResolver for MockedConfig {
        type Output = MetaAgentConfig;
        type Error = serde_yaml::Error;

        fn resolve(self) -> Result<Self::Output, Self::Error> {
            serde_yaml::from_str::<Self::Output>(self)
        }
    }

    #[test]
    fn resolve_one_agent() {
        let actual = MockedConfig::resolve(
            r#"
                # just Infra Agent enabled
                agents:
                    nr_infra_agent:
            "#,
        )
        .unwrap();
        let expected = MetaAgentConfig {
            agents: [(
                AgentType::InfraAgent(None),
                config::Value::new(None, config::ValueKind::Nil),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_two_different_agents() {
        // Build the config
        let actual = MockedConfig::resolve(
            r#"
        # both enabled
        agents:
            nr_infra_agent:
            nr_otel_collector:
        "#,
        )
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
        let actual = MockedConfig::resolve(
            r#"
        # both enabled
        agents:
            nr_infra_agent:
            nr_otel_collector:
            nr_infra_agent/otherinstance:
        "#,
        )
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

    // FIXME: This test is redundant
    #[test]
    fn resolve_agents_with_custom_configs() {
        // Build the config
        let actual = MockedConfig::resolve(
            r#"
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
        "#,
        )
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
        let actual = MockedConfig::resolve(
            r#"
        # just Infra Agent enabled
        agents:
            nr_infra_agent:
        this_is_another_random_config: value
        "#,
        );
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("unknown field `this_is_another_random_config`"));
    }

    #[test]
    fn resolve_empty_agents_field() {
        let actual = MockedConfig::resolve("agents:");
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("config must contain at least one agent"));
    }

    #[test]
    fn resolve_custom_agent_with_invalid_config() {
        let actual = MockedConfig::resolve(
            r#"
        # just Infra Agent enabled
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
        );
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("custom agent type `custom_agent/nobin` must have a `bin` key"));
    }
}
