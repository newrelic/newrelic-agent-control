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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Write};

    use tempfile::NamedTempFile;

    use crate::config::{
        agent_configs::{OpAMPClientConfig, SuperAgentConfig},
        loader::{SuperAgentConfigLoader, SuperAgentConfigLoaderFile},
    };

    #[test]
    fn load_empty_agents_field_good() {
        let mut tmp_file = NamedTempFile::new().unwrap();
        let sample_config = r#"
agents: {}
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        write!(tmp_file, "{}", sample_config).unwrap();

        let actual = SuperAgentConfigLoaderFile::new(tmp_file.path()).load_config();

        let expected = SuperAgentConfig {
            agents: HashMap::new(),
            opamp: Some(OpAMPClientConfig {
                endpoint: "http://127.0.0.1/v1/opamp".to_string(),
                headers: None,
            }),
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
