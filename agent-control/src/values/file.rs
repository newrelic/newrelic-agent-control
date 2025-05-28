use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{
    AGENT_CONTROL_CONFIG_FILENAME, SUB_AGENT_DIR, VALUES_DIR, VALUES_FILENAME,
};
use crate::opamp::remote_config::hash::{ConfigState, Hash};
use crate::values::config::{Config, RemoteConfig};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::has_remote_management;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use opamp_client::operation::capabilities::Capabilities;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use thiserror::Error;
use tracing::log::trace;
use tracing::{debug, error};

#[cfg(target_family = "unix")]
pub const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

enum ConfigOrigin {
    Local,
    Remote,
}

#[derive(Error, Debug)]
pub enum OnHostConfigRepositoryError {
    #[error("serialize error loading SubAgentConfig: `{0}`")]
    StoreSerializeError(#[from] serde_yaml::Error),
    #[error("directory manager error: `{0}`")]
    DirectoryManagementError(#[from] DirectoryManagementError),
    #[error("file write error: `{0}`")]
    WriteError(#[from] WriteError),
    #[error("file read error: `{0}`")]
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
        ConfigRepositoryFile {
            directory_manager: DirectoryManagerFs {},
            file_rw: LocalFile,
            remote_conf_path: remote_path,
            local_conf_path: local_path,
            remote_enabled: false,
            rw_lock: RwLock::new(()),
        }
    }

    pub fn with_remote(mut self) -> Self {
        self.remote_enabled = true;
        self
    }
}

impl<F, S> ConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn get_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        if agent_id.is_agent_control_id() {
            return self.local_conf_path.join(AGENT_CONTROL_CONFIG_FILENAME);
        }
        concatenate_sub_agent_dir_path(&self.local_conf_path, agent_id)
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        // This file (soon files) will often be removed, but its parent directory contains files
        // that should persist across these deletions.
        if agent_id.is_agent_control_id() {
            return self.remote_conf_path.join(AGENT_CONTROL_CONFIG_FILENAME);
        }
        concatenate_sub_agent_dir_path(&self.remote_conf_path, agent_id)
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(
        &self,
        path: PathBuf,
        origin: ConfigOrigin,
    ) -> Result<Option<Config>, OnHostConfigRepositoryError> {
        let values_result = self.file_rw.read(path.as_path());
        match values_result {
            Err(FileReaderError::FileNotFound(e)) => {
                trace!("file not found! {}", e);
                //actively fallback to load local file
                Ok(None)
            }
            Ok(res) => match origin {
                ConfigOrigin::Local => Ok(Some(Config::LocalConfig(serde_yaml::from_str(&res)?))),
                ConfigOrigin::Remote => Ok(Some(Config::RemoteConfig(serde_yaml::from_str(&res)?))),
            },
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
            self.directory_manager.create(
                values_dir_path.as_path(),
                Permissions::from_mode(DIRECTORY_PERMISSIONS),
            )?;
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
        let local_values_path = self.get_values_file_path(agent_id);

        self.load_file_if_present(local_values_path, ConfigOrigin::Local)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))
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

        self.load_file_if_present(remote_values_path, ConfigOrigin::Remote)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))
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
            .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;

        let content = serde_yaml::to_string(remote_config)
            .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;

        self.file_rw
            .write(
                values_file_path.clone().as_path(),
                content,
                Permissions::from_mode(FILE_PERMISSIONS),
            )
            .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;

        Ok(())
    }

    fn get_hash(&self, agent_id: &AgentID) -> Result<Option<Hash>, ConfigRepositoryError> {
        let _read_guard = self.rw_lock.read().unwrap();
        let remote_values_path = self.get_remote_values_file_path(agent_id);

        let maybe_remote = self
            .load_file_if_present(remote_values_path, ConfigOrigin::Remote)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

        if let Some(Config::RemoteConfig(remote_config)) = maybe_remote {
            return Ok(Some(remote_config.config_hash));
        }

        Ok(None)
    }

    fn update_hash_state(&self, agent_id: &AgentID, state: &ConfigState) -> Result<(), ConfigRepositoryError> {
        debug!(
            agent_id = agent_id.to_string(),
            "updating remote config hash"
        );

        let _read_guard = self.rw_lock.read().unwrap();
        let remote_values_path = self.get_remote_values_file_path(agent_id);

        let maybe_remote = self
            .load_file_if_present(remote_values_path.clone(), ConfigOrigin::Remote)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

        if let Some(Config::RemoteConfig(mut remote_config)) = maybe_remote {
            remote_config.config_hash.update_state(state);

            let content = serde_yaml::to_string(&remote_config)
                .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;

            self.file_rw
                .write(
                    remote_values_path.as_path(),
                    content,
                    Permissions::from_mode(FILE_PERMISSIONS),
                )
                .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;

            Ok(())
        } else {
            Err(ConfigRepositoryError::UpdateHashStateError)
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
            std::fs::remove_file(remote_path_file)
                .map_err(|e| ConfigRepositoryError::DeleteError(e.to_string()))?;
        }

        Ok(())
    }
}

pub fn concatenate_sub_agent_dir_path(dir: &Path, agent_id: &AgentID) -> PathBuf {
    dir.join(SUB_AGENT_DIR)
        .join(agent_id)
        .join(VALUES_DIR)
        .join(VALUES_FILENAME)
}

#[cfg(test)]
pub mod tests {
    use rstest::*;

    use super::{ConfigRepositoryFile, concatenate_sub_agent_dir_path};
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::opamp::remote_config::hash::Hash;
    use crate::values;
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::{
        ConfigRepository, ConfigRepositoryError, load_remote_fallback_local,
    };
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagementError::ErrorCreatingDirectory;
    use fs::directory_manager::DirectoryManager;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::file_reader::FileReader;
    use fs::mock::MockLocalFile;
    use fs::writer_file::FileWriter;
    use serde_yaml::Value;
    use std::collections::HashMap;
    use std::fs::Permissions;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
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
        let remote_dir_path = Path::new("some/remote/path");
        let local_dir_path = Path::new("some/local/path");

        ConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_dir_path,
            remote_dir_path,
            remote_enabled,
        )
    }

    fn get_conf_path(
        yaml_config: &ConfigRepositoryFile<MockLocalFile, MockDirectoryManager>,
    ) -> &Path {
        if yaml_config.remote_enabled {
            &yaml_config.remote_conf_path
        } else {
            &yaml_config.local_conf_path
        }
    }

    #[fixture]
    fn agent_id() -> AgentID {
        AgentID::new("some-agent-id").unwrap()
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
            concatenate_sub_agent_dir_path(get_conf_path(&repo), &agent_id).as_path(),
            yaml_config_content.to_string(),
        );

        let config = load_remote_fallback_local(&repo, &agent_id, &default_capabilities())
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

        repo.file_rw.should_not_read_file_not_found(
            concatenate_sub_agent_dir_path(&repo.remote_conf_path, &agent_id).as_path(),
            "some_error_message".to_string(),
        );

        let yaml_config_content = "some_config: true\nanother_item: false";
        repo.file_rw.should_read(
            concatenate_sub_agent_dir_path(&repo.local_conf_path, &agent_id).as_path(),
            yaml_config_content.to_string(),
        );

        let config = load_remote_fallback_local(&repo, &agent_id, &default_capabilities())
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
        repo.file_rw.should_not_read_file_not_found(
            concatenate_sub_agent_dir_path(&repo.local_conf_path, &agent_id).as_path(),
            "some message".to_string(),
        );

        let yaml_config =
            load_remote_fallback_local(&repo, &agent_id, &default_capabilities()).unwrap();

        assert!(yaml_config.is_none());
    }

    #[rstest]
    #[case::remote_enabled(true)]
    #[case::remote_disabled(false)]
    fn test_load_io_error(#[case] remote_enabled: bool, agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(remote_enabled);
        repo.file_rw.should_not_read_io_error(
            concatenate_sub_agent_dir_path(get_conf_path(&repo), &agent_id).as_path(),
        );

        let result = load_remote_fallback_local(&repo, &agent_id, &default_capabilities());
        let err = result.unwrap_err();
        assert_matches!(err, ConfigRepositoryError::LoadError(s) => {
            assert!(s.contains("file read error"));
        });
    }

    #[rstest]
    fn test_store_remote(agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(false);

        repo.directory_manager.should_create(
            Path::new("some/remote/path/fleet/agents.d/some-agent-id/values"),
            Permissions::from_mode(0o700),
        );

        repo.file_rw.should_write(
            concatenate_sub_agent_dir_path(&repo.remote_conf_path, &agent_id).as_path(),
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig::new(yaml_config, Hash::new("a-hash".to_string()));
        repo.store_remote(&agent_id, &remote_config).unwrap();
    }

    #[rstest]
    fn test_store_remote_error_creating_dir(agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(false);

        repo.directory_manager.should_not_create(
            Path::new("some/remote/path/fleet/agents.d/some-agent-id/values"),
            Permissions::from_mode(0o700),
            ErrorCreatingDirectory("dir name".to_string(), "oh now...".to_string()),
        );

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig::new(yaml_config, Hash::new("a-hash".to_string()));
        let result = repo.store_remote(&agent_id, &remote_config);
        let err = result.unwrap_err();
        assert_matches!(err, ConfigRepositoryError::StoreError(s) => {
            assert!(s.contains("cannot create directory"));
        });
    }

    #[rstest]
    fn test_store_remote_error_writing_file(agent_id: AgentID) {
        let mut repo = yaml_config_repository_file_mock(false);

        repo.directory_manager.should_create(
            Path::new("some/remote/path/fleet/agents.d/some-agent-id/values"),
            Permissions::from_mode(0o700),
        );

        repo.file_rw.should_not_write(
            concatenate_sub_agent_dir_path(&repo.remote_conf_path, &agent_id).as_path(),
            "config:\n  one_item: one value\nhash: a-hash\nstate: applying\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));
        let remote_config = RemoteConfig::new(yaml_config, Hash::new("a-hash".to_string()));
        let result = repo.store_remote(&agent_id, &remote_config);
        let err = result.unwrap_err();
        assert_matches!(err, ConfigRepositoryError::StoreError(s) => {
            assert!(s.contains("error creating file"));
        });
    }

    #[rstest]
    fn test_delete_remote(agent_id: AgentID) {
        // TODO add a test without mocks checking actual deletion
        let repo = yaml_config_repository_file_mock(false);
        repo.delete_remote(&agent_id).unwrap();
    }
}
