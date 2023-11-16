use crate::config::super_agent_configs::SuperAgentConfig;
use crate::{config::error::SuperAgentConfigError, super_agent::defaults::SUPER_AGENT_DATA_DIR};
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::warn;

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
    fn delete(&self) -> Result<(), SuperAgentConfigError>;
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

    //TODO this code is not unit tested
    fn delete(&self) -> Result<(), SuperAgentConfigError> {
        if let Some(remote_path_file) = &self.remote_path {
            let _ = fs::remove_file(remote_path_file);
            Ok(())
        } else {
            unreachable!("we should not write into local paths")
        }
    }
}

impl SuperAgentConfigStoreFile {
    pub fn new(file_path: &Path) -> Self {
        Self {
            local_path: file_path.to_path_buf(),
            remote_path: None,
        }
    }

    pub fn with_remote(self) -> Result<Self, SuperAgentConfigStoreError> {
        let remote_path = format!("{}/{}", SUPER_AGENT_DATA_DIR, "config.yaml");
        // create and open the file in read-write mode even if does not exists
        let _ = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&remote_path)?;

        Ok(Self {
            local_path: self.local_path,
            remote_path: Some(Path::new(&remote_path).to_path_buf()),
        })
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigStoreError> {
        let local_config_file = std::fs::File::open(&self.local_path)?;
        let mut local_config: SuperAgentConfig = serde_yaml::from_reader(local_config_file)?;

        if let Some(remote_config_file) = &self.remote_path {
            if remote_config_file.as_path().exists() {
                let remote_config_file = std::fs::File::open(remote_config_file)?;
                let remote_config = serde_yaml::from_reader(remote_config_file)
                    .map_err(|err| warn!("Unable to parse remote config: {}", err))
                    .ok();

                if let Some(remote_config) = remote_config {
                    // replace local agents with remote ones
                    local_config.agents = remote_config;
                }
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
                fs::File::create(remote_path_file)?,
                sub_agents,
            )?)
        } else {
            unreachable!("we should not write into local paths")
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
            fn delete(&self) -> Result<(), super::SuperAgentConfigError>;
        }
    }

    impl MockSubAgentsConfigStore {
        pub fn should_load(&mut self, sub_agents_config: &SubAgentsConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_load()
                .once()
                .returning(move || Ok(sub_agents_config.clone()));
        }
    }

    #[test]
    fn load_agents_local_remote() {
        use super::SuperAgentConfigStore;

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

        let mut store = SuperAgentConfigStoreFile::new(local_file.path());

        store.remote_path = Some(remote_file.path().to_path_buf());

        let actual = SuperAgentConfigStore::load(&store);

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
