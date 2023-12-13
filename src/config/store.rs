use crate::config::super_agent_configs::SuperAgentConfig;
use crate::{config::error::SuperAgentConfigError, super_agent::defaults::SUPER_AGENT_DATA_DIR};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
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

pub trait SuperAgentConfigStore: SuperAgentConfigLoader + SuperAgentConfigStorer {}

pub trait SuperAgentConfigStorer {
    fn store(&self, config: SuperAgentConfig) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

#[cfg_attr(test, mockall::automock)]
pub trait SuperAgentConfigLoader {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

pub trait SubAgentsConfigStore {
    fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
    fn store(&self, config: &SubAgentsConfig) -> Result<(), SuperAgentConfigError>;
    fn delete(&self) -> Result<(), SuperAgentConfigError>;
}

pub struct SuperAgentConfigStoreFile {
    local_path: PathBuf,
    remote_path: Option<PathBuf>,
    rw_lock: RwLock<()>,
}

impl SuperAgentConfigLoader for SuperAgentConfigStoreFile {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
}

impl SuperAgentConfigStore for SuperAgentConfigStoreFile {}

impl SuperAgentConfigStorer for SuperAgentConfigStoreFile {
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
        let Some(remote_path_file) = &self.remote_path else {
            unreachable!("we should not write into local paths");
        };
        let _write_guard = self.rw_lock.write().unwrap();
        if remote_path_file.exists() {
            fs::remove_file(remote_path_file)?;
        }
        Ok(())
    }
}

impl SuperAgentConfigStoreFile {
    pub fn new(file_path: &Path) -> Self {
        Self {
            local_path: file_path.to_path_buf(),
            remote_path: None,
            rw_lock: RwLock::new(()),
        }
    }

    pub fn with_remote(self) -> Result<Self, SuperAgentConfigStoreError> {
        let remote_path = format!("{}/{}", SUPER_AGENT_DATA_DIR, "config.yaml");

        Ok(Self {
            local_path: self.local_path,
            remote_path: Some(Path::new(&remote_path).to_path_buf()),
            rw_lock: RwLock::new(()),
        })
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigStoreError> {
        let _read_guard = self.rw_lock.read().unwrap();
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
        //TODO we should inject DirectoryManager and ensure the directory exists
        let _write_guard = self.rw_lock.write().unwrap();
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
        store::{SuperAgentConfigLoader, SuperAgentConfigStoreFile},
        super_agent_configs::{
            AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SubAgentsConfig,
            SuperAgentConfig,
        },
    };

    use mockall::{mock, predicate};

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

        pub fn should_store(&mut self, sub_agents_config: &SubAgentsConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_store()
                .once()
                .with(predicate::eq(sub_agents_config))
                .returning(move |_| Ok(()));
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

        let actual = SuperAgentConfigLoader::load(&store);

        let expected = SuperAgentConfig {
            agents: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
                endpoint: "http://127.0.0.1/v1/opamp".to_string(),
                headers: None,
            }),
            k8s: None,
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
