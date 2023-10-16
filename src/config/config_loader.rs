use crate::config::agent_configs::SuperAgentConfig;
use crate::config::error::SuperAgentConfigError;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SuperAgentConfigLoaderError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
}

pub trait SuperAgentConfigLoader {
    fn load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

pub struct SuperAgentConfigLoaderFile {
    file_path: PathBuf,
}

impl SuperAgentConfigLoader for SuperAgentConfigLoaderFile {
    fn load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
}

impl SuperAgentConfigLoaderFile {
    pub fn new(file_path: &Path) -> Self {
        Self {
            file_path: file_path.to_path_buf(),
        }
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigLoaderError> {
        let f = std::fs::File::open(&self.file_path)?;
        let d: SuperAgentConfig = serde_yaml::from_reader(f)?;
        Ok(d)
    }
}
