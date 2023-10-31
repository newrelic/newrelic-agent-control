use crate::config::error::SuperAgentConfigError;
use crate::config::super_agent_configs::SuperAgentConfig;
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::super_agent_configs::SubAgentsConfig;

#[derive(Error, Debug)]
pub enum SuperAgentConfigLoaderError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
}

pub trait SuperAgentConfigLoader: SubAgentsConfigLoader {
    fn load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

pub trait SubAgentsConfigLoader {
    fn load_config(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
}

pub struct SuperAgentConfigLoaderFile {
    local_path: PathBuf,
    remote_path: PathBuf,
}

impl SuperAgentConfigLoader for SuperAgentConfigLoaderFile {
    fn load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
}

impl SubAgentsConfigLoader for SuperAgentConfigLoaderFile {
    fn load_config(&self) -> Result<SubAgentsConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.agents)
    }
}

impl SuperAgentConfigLoaderFile {
    pub fn new(file_path: &Path, remote_path: &Path) -> Self {
        Self {
            local_path: file_path.to_path_buf(),
            remote_path: remote_path.to_path_buf(),
        }
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigLoaderError> {
        let local_config_file = std::fs::File::open(&self.local_path)?;
        let mut local_config: SuperAgentConfig = serde_yaml::from_reader(local_config_file)?;

        let remote_config_file = std::fs::File::open(&self.remote_path)?;
        let remote_config: SuperAgentConfig = serde_yaml::from_reader(remote_config_file)?;

        // replace local agents with remote ones
        if !remote_config.agents.is_empty() {
            local_config.agents = remote_config.agents;
        }

        Ok(local_config)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Write};

    use tempfile::NamedTempFile;

    use crate::config::{
        loader::{SuperAgentConfigLoader, SuperAgentConfigLoaderFile},
        super_agent_configs::{
            AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SuperAgentConfig,
        },
    };

    #[test]
    fn load_empty_agents_field_good() {
        let mut local_file = NamedTempFile::new().unwrap();
        let local_config = r#"
agents: {}
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        write!(local_file, "{}", local_config).unwrap();

        let mut remote_file = NamedTempFile::new().unwrap();
        let remote_config = r#"
agents:
  rolldice:
    agent_type: "com.newrelic.infrastructure_agent:0.0.2"
"#;
        write!(remote_file, "{}", remote_config).unwrap();

        let actual =
            SuperAgentConfigLoaderFile::new(local_file.path(), remote_file.path()).load_config();

        let expected = SuperAgentConfig {
            agents: HashMap::from([(
                AgentID::new("rolldice"),
                SubAgentConfig {
                    agent_type: AgentTypeFQN("com.newrelic.infrastructure_agent:0.0.2".to_string()),
                    values_file: None,
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
                endpoint: "http://127.0.0.1/v1/opamp".to_string(),
                headers: None,
            }),
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
