use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{
    FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    STORE_KEY_OPAMP_DATA_CONFIG,
};
use crate::opamp::instance_id::on_host::storer::build_config_name;
use crate::opamp::remote_config::hash::ConfigState;
use crate::values::config::{Config, RemoteConfig};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::has_remote_management;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use opamp_client::operation::capabilities::Capabilities;
use std::path::PathBuf;
use std::sync::RwLock;
use thiserror::Error;
use tracing::log::trace;
use tracing::{debug, error};

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
    directory_manager: S,
    file_rw: F,
    remote_conf_path: PathBuf,
    local_conf_path: PathBuf,
    remote_enabled: bool,
    rw_lock: RwLock<()>,
}

impl ConfigRepositoryFile<LocalFile, DirectoryManagerFs> {
    pub fn new(local_path: PathBuf, remote_path: PathBuf) -> Self {
        Self {
            directory_manager: DirectoryManagerFs {},
            file_rw: LocalFile,
            remote_conf_path: remote_path,
            local_conf_path: local_path,
            remote_enabled: false,
            rw_lock: RwLock::new(()),
        }
    }

    pub fn with_remote(self) -> Self {
        Self {
            remote_enabled: true,
            ..self
        }
    }
}

impl<F, S> ConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn get_local_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.local_conf_path
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG))
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.remote_conf_path
            .join(FOLDER_NAME_FLEET_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG))
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(
        &self,
        path: PathBuf,
    ) -> Result<Option<String>, OnHostConfigRepositoryError> {
        let values_result = self.file_rw.read(path.as_path());
        match values_result {
            Err(FileReaderError::FileNotFound(e)) => {
                trace!("file not found! {e}");
                //actively fallback to load local file
                Ok(None)
            }
            Ok(res) => Ok(Some(res)),
            Err(err) => {
                // we log any unexpected error for now but maybe we should propagate it
                error!("error loading remote file {}", path.display());
                Err(err.into())
            }
        }
    }

    /// ensures directory exists
    fn ensure_directory_existence(
        &self,
        values_file_path: &PathBuf,
    ) -> Result<(), OnHostConfigRepositoryError> {
        let mut values_dir_path = PathBuf::from(&values_file_path);
        values_dir_path.pop();

        if !values_dir_path.exists() {
            self.directory_manager.create(values_dir_path.as_path())?;
        }
        Ok(())
    }
}

impl<F, S> ConfigRepository for ConfigRepositoryFile<F, S>
where
    S: DirectoryManager + Send + Sync + 'static,
    F: FileWriter + FileReader + Send + Sync + 'static,
{
    #[tracing::instrument(skip_all, err)]
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError> {
        let _read_guard = self.rw_lock.read().unwrap();
        let local_values_path = self.get_local_values_file_path(agent_id);

        self.load_file_if_present(local_values_path)
            .map_err(|err| ConfigRepositoryError::LoadError(format!("loading local config: {err}")))
            .and_then(|maybe_values| {
                maybe_values.map_or(Ok(None), |values| {
                    serde_yaml::from_str(&values)
                        .map(Config::LocalConfig)
                        .map(Some)
                        .map_err(|err| {
                            ConfigRepositoryError::LoadError(format!("loading local config: {err}"))
                        })
                })
            })
    }

    #[tracing::instrument(skip_all, err)]
    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            return Ok(None);
        }
        let _read_guard = self.rw_lock.read().unwrap();
        let remote_values_path = self.get_remote_values_file_path(agent_id);

        self.load_file_if_present(remote_values_path)
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("loading remote config: {err}"))
            })
            .and_then(|maybe_values| {
                maybe_values.map_or(Ok(None), |values| {
                    serde_yaml::from_str(&values)
                        .map(Config::RemoteConfig)
                        .map(Some)
                        .map_err(|err| {
                            ConfigRepositoryError::LoadError(format!(
                                "loading remote config: {err}"
                            ))
                        })
                })
            })
    }

    #[tracing::instrument(skip_all, err)]
    fn store_remote(
        &self,
        agent_id: &AgentID,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError> {
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();

        let values_file_path = self.get_remote_values_file_path(agent_id);

        self.ensure_directory_existence(&values_file_path)
            .map_err(|err| {
                ConfigRepositoryError::StoreError(format!(
                    "ensuring the directory for storing remote config exists: {err}"
                ))
            })?;

        let content = serde_yaml::to_string(remote_config).map_err(|err| {
            ConfigRepositoryError::StoreError(format!("storing remote config: {err}"))
        })?;

        self.file_rw
            .write(values_file_path.clone().as_path(), content)
            .map_err(|err| {
                ConfigRepositoryError::StoreError(format!("storing remote config: {err}"))
            })?;

        Ok(())
    }

    fn get_remote_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<RemoteConfig>, ConfigRepositoryError> {
        let _read_guard = self.rw_lock.read().unwrap();
        let remote_values_path = self.get_remote_values_file_path(agent_id);

        // If there is a remote we try to deserialize it with serde_yaml into a RemoteConfig struct
        self.load_file_if_present(remote_values_path)
            // maps an error during the file loading into the right error
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("getting remote config hash: {err}"))
            })
            .and_then(|maybe_values| {
                maybe_values
                    .map(|values| {
                        serde_yaml::from_str(&values)
                            // maps an error during the serde_yaml deserializing into the right error
                            .map_err(|err| {
                                ConfigRepositoryError::LoadError(format!(
                                    "getting remote config hash: {err}"
                                ))
                            })
                    })
                    .transpose()
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

        let _read_guard = self.rw_lock.read().unwrap();
        let remote_values_path = self.get_remote_values_file_path(agent_id);

        let maybe_remote = self
            .load_file_if_present(remote_values_path.clone())
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("updating remote config state: {err}"))
            })
            .and_then(|maybe_values| {
                maybe_values.map_or(Ok(None), |values| {
                    serde_yaml::from_str(&values)
                        .map(Config::RemoteConfig)
                        .map(Some)
                        .map_err(|err| {
                            ConfigRepositoryError::LoadError(format!(
                                "updating remote config state: {err}"
                            ))
                        })
                })
            })?;

        if let Some(Config::RemoteConfig(remote_config)) = maybe_remote {
            let content =
                serde_yaml::to_string(&remote_config.with_state(state)).map_err(|err| {
                    ConfigRepositoryError::StoreError(format!(
                        "updating remote config state: {err}"
                    ))
                })?;

            self.file_rw
                .write(remote_values_path.as_path(), content)
                .map_err(|err| {
                    ConfigRepositoryError::StoreError(format!(
                        "updating remote config state: {err}"
                    ))
                })?;

            Ok(())
        } else {
            Err(ConfigRepositoryError::UpdateHashStateError(
                "No remote config found".to_string(),
            ))
        }
    }

    // TODO Currently we are not deleting the whole folder, therefore multiple files are not supported
    // Moreover, we are also loading one file only, therefore we should review this once support is added
    // Notice that in that case we will likely need to move AgentControlConfig file to a folder
    #[tracing::instrument(skip_all err)]
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();

        let remote_path_file = self.get_remote_values_file_path(agent_id);
        if remote_path_file.exists() {
            debug!("deleting remote config: {:?}", remote_path_file);
            std::fs::remove_file(remote_path_file).map_err(|e| {
                ConfigRepositoryError::DeleteError(format!("deleting remote config: {e}"))
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use rstest::*;

    use super::ConfigRepositoryFile;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::values;
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagementError::ErrorCreatingDirectory;
    use fs::directory_manager::DirectoryManager;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::file_reader::FileReader;
    use fs::mock::MockLocalFile;
    use fs::writer_file::FileWriter;
    use serde_yaml::Value;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::RwLock;
    use values::yaml_config::YAMLConfig;

    impl<F, S> ConfigRepositoryFile<F, S>
    where
        S: DirectoryManager,
        F: FileWriter + FileReader,
    {
        pub fn with_mocks(
            file_rw: F,
            directory_manager: S,
            local_path: &Path,
            remote_path: &Path,
            remote_enabled: bool,
        ) -> Self {
            ConfigRepositoryFile {
                file_rw,
                directory_manager,
                remote_conf_path: remote_path.to_path_buf(),
                local_conf_path: local_path.to_path_buf(),
                remote_enabled,
                rw_lock: RwLock::new(()),
            }
        }
    }

    fn yaml_config_repository_file_mock(
        remote_enabled: bool,
    ) -> ConfigRepositoryFile<MockLocalFile, MockDirectoryManager> {
        let file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManager::new();
        let remote_dir_path = Path::new("some/remote/path/");
        let local_dir_path = Path::new("some/local/path/");

        ConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_dir_path,
            remote_dir_path,
            remote_enabled,
        )
    }

    fn get_test_path_to_read(
        repo: &ConfigRepositoryFile<MockLocalFile, MockDirectoryManager>,
        agent_id: &AgentID,
        remote_enabled: bool,
    ) -> PathBuf {
        if remote_enabled {
            repo.get_remote_values_file_path(agent_id)
        } else {
            repo.get_local_values_file_path(agent_id)
        }
    }

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

        let mut repo = yaml_config_repository_file_mock(remote_enabled);

        repo.file_rw.should_read(
            &get_test_path_to_read(&repo, &agent_id, remote_enabled),
            yaml_config_content.to_string(),
        );

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
        let mut repo = yaml_config_repository_file_mock(true);

        let remote_path = repo.get_remote_values_file_path(&agent_id);
        let local_path = repo.get_local_values_file_path(&agent_id);

        repo.file_rw
            .should_not_read_file_not_found(&remote_path, "some_error_message".to_string());

        let yaml_config_content = "some_config: true\nanother_item: false";
        repo.file_rw
            .should_read(&local_path, yaml_config_content.to_string());

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
        let mut repo = yaml_config_repository_file_mock(false);

        let local_path = repo.get_local_values_file_path(&agent_id);
        repo.file_rw
            .should_not_read_file_not_found(&local_path, "some message".to_string());

        let yaml_config = repo
            .load_remote_fallback_local(&agent_id, &default_capabilities())
            .unwrap();

        assert!(yaml_config.is_none());
    }

    #[rstest]
    #[case::remote_enabled(true)]
    #[case::remote_disabled(false)]
    fn test_load_io_error(#[case] remote_enabled: bool, agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(remote_enabled);

        repo.file_rw
            .should_not_read_io_error(&get_test_path_to_read(&repo, &agent_id, remote_enabled));

        let result = repo.load_remote_fallback_local(&agent_id, &default_capabilities());
        let err = result.unwrap_err();
        assert_matches!(err, ConfigRepositoryError::LoadError(s) => {
            assert!(s.contains("file read error"));
        });
    }

    #[rstest]
    fn test_store_remote(agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(false);

        let remote_path = repo.get_remote_values_file_path(&agent_id);

        repo.directory_manager
            .should_create(remote_path.parent().unwrap());

        repo.file_rw.should_write(
            &remote_path,
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
        );

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
        let mut repo = yaml_config_repository_file_mock(false);

        let remote_path = repo.get_remote_values_file_path(&agent_id);

        repo.directory_manager.should_not_create(
            remote_path.parent().unwrap(),
            ErrorCreatingDirectory("dir name".to_string(), "oh now...".to_string()),
        );

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
        let mut repo = yaml_config_repository_file_mock(false);

        let remote_path = repo.get_remote_values_file_path(&agent_id);

        repo.directory_manager
            .should_create(remote_path.parent().unwrap());

        repo.file_rw.should_not_write(
            &remote_path,
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
        );

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
        let repo = yaml_config_repository_file_mock(false);
        repo.delete_remote(&agent_id).unwrap();
    }
}
