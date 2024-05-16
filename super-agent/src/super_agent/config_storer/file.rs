use crate::super_agent::config::{
    SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use crate::super_agent::config_storer::storer::{
    SuperAgentConfigLoader, SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
    SuperAgentDynamicConfigStorer,
};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::warn;

#[derive(thiserror::Error, Debug)]
pub enum ConfigStoreError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
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

impl SuperAgentDynamicConfigLoader for SuperAgentConfigStoreFile {
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl SuperAgentDynamicConfigDeleter for SuperAgentConfigStoreFile {
    //TODO this code is not unit tested
    fn delete(&self) -> Result<(), SuperAgentConfigError> {
        let Some(remote_path_file) = &self.remote_path else {
            unreachable!("we should not write into local paths");
        };
        // clippy complains because we are not changing the lock's content
        // TODO: check RwLock is being used efficiently for this use-case.
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();
        if remote_path_file.exists() {
            std::fs::remove_file(remote_path_file)?;
        }
        Ok(())
    }
}

impl SuperAgentDynamicConfigStorer for SuperAgentConfigStoreFile {
    fn store(&self, sub_agents: &SuperAgentDynamicConfig) -> Result<(), SuperAgentConfigError> {
        //TODO we should inject DirectoryManager and ensure the directory exists
        // clippy complains because we are not changing the lock's content
        // TODO: check RwLock is being used efficiently for this use-case.
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();
        let Some(remote_path_file) = &self.remote_path else {
            unreachable!("we should not write into local paths");
        };
        Ok(serde_yaml::to_writer(
            std::fs::File::create(remote_path_file)?,
            sub_agents,
        )?)
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

    // with_remote is supported for onhost implementation only and to make sure it is not used
    // we avoid to compile it for k8s
    #[cfg(feature = "onhost")]
    pub fn with_remote(self) -> Self {
        let remote_path = format!(
            "{}/{}",
            crate::super_agent::defaults::SUPER_AGENT_DATA_DIR(),
            "config.yaml"
        );

        Self {
            remote_path: Some(Path::new(&remote_path).to_path_buf()),
            ..self
        }
    }

    pub fn config_path(&self) -> &Path {
        self.remote_path.as_ref().unwrap_or(&self.local_path)
    }

    fn _load_config(&self) -> Result<SuperAgentConfig, ConfigStoreError> {
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
                    local_config.dynamic = remote_config;
                }
            }
        }

        Ok(local_config)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::super_agent::config::{
        AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SuperAgentConfig,
    };
    use http::HeaderMap;
    use std::{collections::HashMap, io::Write};
    use tempfile::NamedTempFile;
    use url::Url;

    #[test]
    fn load_agents_local_remote() {
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
    agent_type: "namespace/com.newrelic.infrastructure_agent:0.0.2"
"#;
        write!(remote_file, "{}", remote_config).unwrap();

        let mut store = SuperAgentConfigStoreFile::new(local_file.path());

        store.remote_path = Some(remote_file.path().to_path_buf());

        let actual = SuperAgentConfigLoader::load(&store);

        let expected = SuperAgentConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "namespace/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
                endpoint: Url::try_from("http://127.0.0.1/v1/opamp").unwrap(),
                headers: HeaderMap::default(),
            }),
            k8s: None,
            ..Default::default()
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
