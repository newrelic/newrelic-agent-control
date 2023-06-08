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
