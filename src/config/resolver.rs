use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::SuperAgentConfig, error::SuperAgentConfigError};

/// Builder for the configuration, managing if it is loaded from a default expected file or from
/// a custom one provided by the command line arguments.
pub struct Resolver(ConfigBuilder<DefaultState>);

impl Resolver {
    // build from an arbitrary source only for tests
    #[cfg(test)]
    fn new<T>(source: T) -> Self
    where
        T: config::Source + Send + Sync + 'static,
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

    fn build_config(self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self.0.build()?.try_deserialize::<SuperAgentConfig>()?)
    }

    /// Attempts to build the configuration
    pub fn retrieve_config(file: &Path) -> Result<SuperAgentConfig, SuperAgentConfigError> {
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

    use std::collections::HashMap;

    use super::*;
    use crate::config::{agent_configs::SuperAgentConfig, resolver::Resolver};

    #[test]
    fn resolve_empty_agents_field_should_fail_without_empty_map() {
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
    fn resolve_empty_agents_field_good() {
        let actual = Resolver::new(File::from_str(
            r"
agents: {}
",
            FileFormat::Yaml,
        ))
        .build_config();

        let expected = SuperAgentConfig {
            agents: HashMap::new(),
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
