use crate::config::error::SuperAgentConfigError;
use crate::config::super_agent_configs::SuperAgentConfig;
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::super_agent_configs::SubAgentsConfig;

#[derive(Error, Debug)]
pub enum SuperAgentConfigStoreError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
}

#[derive(Error, Debug)]
pub enum SubAgentsConfigStoreError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
}

pub trait SuperAgentConfigStore: SubAgentsConfigStore {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
    fn store(&self, config: SuperAgentConfig) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

pub trait SubAgentsConfigStore {
    fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
    fn store(&self, config: &SubAgentsConfig) -> Result<(), SuperAgentConfigError>;
}

pub struct SuperAgentConfigStoreFile {
    local_path: PathBuf,
    remote_path: Option<PathBuf>,
}

impl SuperAgentConfigStore for SuperAgentConfigStoreFile {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
    fn store(&self, _config: SuperAgentConfig) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        unimplemented!()
    }
}

impl SubAgentsConfigStore for SuperAgentConfigStoreFile {
    fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.agents)
    }
    fn store(&self, config: &SubAgentsConfig) -> Result<(), SuperAgentConfigError> {
        Ok(self._store_sub_agents_config(config)?)
    }
}

impl SuperAgentConfigStoreFile {
    pub fn new(file_path: &Path) -> Self {
        Self {
            local_path: file_path.to_path_buf(),
            remote_path: None,
        }
    }

    pub fn with_remote(self, remote_path: &Path) -> Self {
        Self {
            local_path: self.local_path,
            remote_path: Some(remote_path.to_path_buf()),
        }
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigStoreError> {
        let local_config_file = std::fs::File::open(&self.local_path)?;
        let mut local_config: SuperAgentConfig = serde_yaml::from_reader(local_config_file)?;

        if let Some(remote_path_file) = &self.remote_path {
            let remote_config_file = std::fs::File::open(remote_path_file)?;
            let remote_config: SuperAgentConfig = serde_yaml::from_reader(remote_config_file)?;

            // replace local agents with remote ones
            if !remote_config.agents.is_empty() {
                local_config.agents = remote_config.agents;
            }
        }

        Ok(local_config)
    }

    fn _store_sub_agents_config(
        &self,
        sub_agents: &SubAgentsConfig,
    ) -> Result<(), SuperAgentConfigStoreError> {
        if let Some(remote_path_file) = &self.remote_path {
            Ok(serde_yaml::to_writer(
                std::fs::File::open(remote_path_file)?,
                sub_agents,
            )?)
        } else {
            Ok(serde_yaml::to_writer(
                std::fs::File::open(&self.local_path)?,
                sub_agents,
            )?)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::HashMap, io::Write};

    use tempfile::NamedTempFile;

    use crate::config::{
        store::SuperAgentConfigStoreFile,
        super_agent_configs::{
            AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SubAgentsConfig,
            SuperAgentConfig,
        },
    };

    use mockall::mock;

    mock! {
        pub SubAgentsConfigStore {}

        impl super::SubAgentsConfigStore for SubAgentsConfigStore {

            fn load(&self) -> Result<super::SubAgentsConfig, super::SuperAgentConfigError>;
            fn store(&self, config: &SubAgentsConfig) -> Result<(), super::SuperAgentConfigError>;
        }
    }

    use super::SuperAgentConfigStore;

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

        let actual = SuperAgentConfigStoreFile::new(local_file.path())
            .with_remote(remote_file.path())
            .load();

        let expected = SuperAgentConfig {
            agents: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN("com.newrelic.infrastructure_agent:0.0.2".to_string()),
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
