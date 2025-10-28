use std::sync::Arc;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::on_host::file_store::FileStore;
use crate::opamp::remote_config::hash::ConfigState;
use crate::values::config::{Config, RemoteConfig};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::{YAMLConfig, has_remote_management};
use fs::directory_manager::{DirectoryManagementError, DirectoryManager};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use opamp_client::operation::capabilities::Capabilities;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum OnHostConfigRepositoryError {
    #[error("serialize error loading SubAgentConfig: {0}")]
    StoreSerializeError(#[from] serde_yaml::Error),
    #[error("directory manager error: {0}")]
    DirectoryManagementError(#[from] DirectoryManagementError),
    #[error("file write error: {0}")]
    WriteError(#[from] WriteError),
    #[error("file read error: {0}")]
    ReadError(#[from] FileReaderError),
    #[cfg(test)]
    #[error("common variant for k8s and on-host implementations")]
    Generic,
}

pub struct ConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_store: Arc<FileStore<F, S>>,
    remote_enabled: bool,
}

impl<F, S> ConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(file_store: Arc<FileStore<F, S>>) -> Self {
        Self {
            file_store,
            remote_enabled: false,
        }
    }

    pub fn with_remote(self) -> Self {
        Self {
            remote_enabled: true,
            ..self
        }
    }
}

impl<F, S> ConfigRepository for ConfigRepositoryFile<F, S>
where
    S: DirectoryManager + Send + Sync + 'static,
    F: FileWriter + FileReader + Send + Sync + 'static,
{
    #[tracing::instrument(skip_all, err)]
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError> {
        self.file_store
            .get_local_data::<YAMLConfig>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| ConfigRepositoryError::LoadError(format!("loading local config: {err}")))
            .map(|opt_yaml| opt_yaml.map(|yc| Config::LocalConfig(yc.into())))
    }

    #[tracing::instrument(skip_all, err)]
    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            Ok(None)
        } else {
            self.file_store
                .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
                .map_err(|err| {
                    ConfigRepositoryError::LoadError(format!("loading remote config: {err}"))
                })
                .map(|opt_rc| opt_rc.map(Config::RemoteConfig))
        }
    }

    #[tracing::instrument(skip_all, err)]
    fn store_remote(
        &self,
        agent_id: &AgentID,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.file_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, remote_config)
            .map_err(|e| ConfigRepositoryError::StoreError(format!("storing remote config: {}", e)))
    }

    fn get_remote_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<RemoteConfig>, ConfigRepositoryError> {
        self.file_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::LoadError(format!("getting remote config hash: {}", e))
            })
    }

    fn update_state(
        &self,
        agent_id: &AgentID,
        state: ConfigState,
    ) -> Result<(), ConfigRepositoryError> {
        debug!(
            agent_id = agent_id.to_string(),
            "updating remote config hash"
        );

        let maybe_config = self
            .file_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::LoadError(format!("updating remote config state: {e}"))
            })?;

        match maybe_config {
            Some(remote_config) => self
                .file_store
                .set_opamp_data(
                    agent_id,
                    STORE_KEY_OPAMP_DATA_CONFIG,
                    &remote_config.with_state(state),
                )
                .map_err(|err| {
                    ConfigRepositoryError::StoreError(format!(
                        "updating remote config state: {err}"
                    ))
                }),
            None => Err(ConfigRepositoryError::UpdateHashStateError(
                "No remote config found".to_string(),
            )),
        }
    }

    // TODO Currently we are not deleting the whole folder, therefore multiple files are not supported
    // Moreover, we are also loading one file only, therefore we should review this once support is added
    // Notice that in that case we will likely need to move AgentControlConfig file to a folder
    #[tracing::instrument(skip_all err)]
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.file_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::DeleteError(format!("deleting remote config: {}", e))
            })
    }
}

#[cfg(test)]
pub mod tests {
    use rstest::*;

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::on_host::file_store::FileStore;
    use crate::on_host::file_store::LocalDir;
    use crate::on_host::file_store::RemoteDir;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::values;
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagementError::ErrorCreatingDirectory;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::mock::MockLocalFile;
    use serde_yaml::Value;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use values::yaml_config::YAMLConfig;

    #[fixture]
    fn agent_id() -> AgentID {
        AgentID::try_from("some-agent-id").unwrap()
    }

    #[rstest]
    #[case::remote_enabled(true)]
    #[case::remote_disabled(false)]
    fn test_load_with(#[case] remote_enabled: bool, agent_id: AgentID) {
        let mut yaml_config_content = "some_config: true\nanother_item: false";
        if remote_enabled {
            yaml_config_content = r#"
config:
    some_config: true
    another_item: false
hash: a-hash
state: applied
"#;
        }

        let mut file_rw = MockLocalFile::new();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let test_path = if remote_enabled {
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
        } else {
            local_dir_path.get_local_file_path(&agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
        };

        // Expectations
        file_rw.should_read(&test_path, yaml_config_content.to_string());

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = if remote_enabled {
            ConfigRepositoryFile::new(file_store).with_remote()
        } else {
            ConfigRepositoryFile::new(file_store)
        };

        let config = repo
            .load_remote_fallback_local(&agent_id, &default_capabilities())
            .expect("unexpected error loading config")
            .expect("expected some configuration, got None");

        assert_eq!(
            config.get_yaml_config().get("some_config").unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            config.get_yaml_config().get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[rstest]
    fn test_load_when_remote_enabled_file_not_found_fallbacks_to_local(agent_id: AgentID) {
        let mut file_rw = MockLocalFile::new();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let remote_path =
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG);
        let local_path = local_dir_path.get_local_file_path(&agent_id, STORE_KEY_LOCAL_DATA_CONFIG);

        // Expectations
        file_rw.should_not_read_file_not_found(&remote_path, "some_error_message".to_string());

        let yaml_config_content = "some_config: true\nanother_item: false";
        file_rw.should_read(&local_path, yaml_config_content.to_string());

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = ConfigRepositoryFile::new(file_store).with_remote();

        let config = repo
            .load_remote_fallback_local(&agent_id, &default_capabilities())
            .expect("unexpected error loading config")
            .expect("expected some configuration, got None");

        assert_eq!(
            config.get_yaml_config().get("some_config").unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            config.get_yaml_config().get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[rstest]
    fn test_load_local_file_not_found_should_return_none(agent_id: AgentID) {
        let mut file_rw = MockLocalFile::new();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = PathBuf::from("some/remote/path/");
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let local_path = local_dir_path.get_local_file_path(&agent_id, STORE_KEY_LOCAL_DATA_CONFIG);

        // Expectations
        file_rw.should_not_read_file_not_found(&local_path, "some message".to_string());

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path,
        ));
        let repo = ConfigRepositoryFile::new(file_store);

        let yaml_config = repo
            .load_remote_fallback_local(&agent_id, &default_capabilities())
            .unwrap();

        assert!(yaml_config.is_none());
    }

    #[rstest]
    #[case::remote_enabled(true)]
    #[case::remote_disabled(false)]
    fn test_load_io_error(#[case] remote_enabled: bool, agent_id: AgentID) {
        let mut file_rw = MockLocalFile::new();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let remote_test_path =
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG);
        let local_test_path =
            local_dir_path.get_local_file_path(&agent_id, STORE_KEY_LOCAL_DATA_CONFIG);

        // Expectations
        if remote_enabled {
            file_rw.should_not_read_io_error(&remote_test_path);
        } else {
            file_rw.should_not_read_io_error(&local_test_path);
        }

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = if remote_enabled {
            ConfigRepositoryFile::new(file_store).with_remote()
        } else {
            ConfigRepositoryFile::new(file_store)
        };

        let result = repo.load_remote_fallback_local(&agent_id, &default_capabilities());
        let err = result.unwrap_err();
        assert_matches!(err, ConfigRepositoryError::LoadError(s) => {
            assert!(s.contains("permission denied")); // the error returned by `should_not_read_io_error`
        });
    }

    #[rstest]
    fn test_store_remote(agent_id: AgentID) {
        let mut file_rw = MockLocalFile::new();
        let mut dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let remote_path =
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG);

        // Expectations
        dir_manager.should_create(remote_path.parent().unwrap());
        file_rw.should_write(
            &remote_path,
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
        );

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));

        let repo = ConfigRepositoryFile::new(file_store);

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig {
            config: yaml_config,
            hash: Hash::from("a-hash"),
            state: ConfigState::Applying,
        };
        repo.store_remote(&agent_id, &remote_config).unwrap();
    }

    #[rstest]
    fn test_store_remote_error_creating_dir(agent_id: AgentID) {
        let file_rw = MockLocalFile::new();
        let mut dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let remote_path =
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG);

        // Expectations
        dir_manager.should_not_create(
            remote_path.parent().unwrap(),
            ErrorCreatingDirectory("dir name".to_string(), "oh now...".to_string()),
        );

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = ConfigRepositoryFile::new(file_store);

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig {
            config: yaml_config,
            hash: Hash::from("a-hash"),
            state: ConfigState::Applying,
        };
        let result = repo.store_remote(&agent_id, &remote_config);
        assert_matches!(result, Err(ConfigRepositoryError::StoreError(_)));
    }

    #[rstest]
    fn test_store_remote_error_writing_file(agent_id: AgentID) {
        let mut file_rw = MockLocalFile::new();
        let mut dir_manager = MockDirectoryManager::new();
        let remote_dir_path = RemoteDir::from(PathBuf::from("some/remote/path/"));
        let local_dir_path = LocalDir::from(PathBuf::from("some/local/path/"));
        let remote_path =
            remote_dir_path.get_remote_file_path(&agent_id, STORE_KEY_OPAMP_DATA_CONFIG);

        // Expectations
        dir_manager.should_create(remote_path.parent().unwrap());
        file_rw.should_not_write(
            &remote_path,
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
        );

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = ConfigRepositoryFile::new(file_store);

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig {
            config: yaml_config,
            hash: Hash::from("a-hash"),
            state: ConfigState::Applying,
        };
        let result = repo.store_remote(&agent_id, &remote_config);
        assert_matches!(result, Err(ConfigRepositoryError::StoreError(_)));
    }

    #[rstest]
    fn test_delete_remote(agent_id: AgentID) {
        // TODO add a test without mocks checking actual deletion
        let file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = PathBuf::from("some/remote/path/");
        let local_dir_path = PathBuf::from("some/local/path/");
        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path,
            remote_dir_path,
        ));
        let repo = ConfigRepositoryFile::new(file_store);
        repo.delete_remote(&agent_id).unwrap();
    }
}
