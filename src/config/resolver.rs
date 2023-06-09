use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::MetaAgentConfig, error::MetaAgentConfigError};

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

    /// Attempts to build the configuration
    pub fn retrieve_config(file: Option<&Path>) -> Result<MetaAgentConfig, MetaAgentConfigError> {
        match file {
            Some(f) => Self::new(f).build_config(),
            None => Self::default().build_config(),
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        let builder =
            Config::builder().add_source(File::new(DEFAULT_STATIC_CONFIG, FileFormat::Yaml));
        Self(builder)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use config::{ConfigError, Source, Value};

    use crate::config::agent_type::AgentType;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct MockedConfig(&'static str);

    impl Source for MockedConfig {
        fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
            unimplemented!()
        }

        fn collect(&self) -> Result<HashMap<String, Value>, ConfigError> {
            serde_yaml::from_str::<HashMap<String, Value>>(self.0)
                .map_err(|e| ConfigError::Message(format!("{}", e)))
        }

        fn collect_to(&self, v: &mut Value) -> Result<(), ConfigError> {
            *v = self.collect()?.into();
            Ok(())
        }
    }

    #[test]
    fn basic_config() {
        let builder = Config::builder().add_source(MockedConfig(
            r#"
                # just Infra Agent enabled
                agents:
                    nr_infra_agent:
            "#,
        ));

        let actual = builder
            .build()
            .unwrap()
            .try_deserialize::<MetaAgentConfig>()
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
}
