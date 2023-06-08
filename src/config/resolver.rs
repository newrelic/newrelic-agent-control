use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::MetaAgentConfig, error::MetaAgentConfigError};

const DEFAULT_STATIC_CONFIG: &str = "/tmp/static.yaml";

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
