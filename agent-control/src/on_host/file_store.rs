use std::{
    io::{self, Error, ErrorKind},
    path::{Path, PathBuf},
    sync::RwLock,
};

use fs::{
    LocalFile,
    directory_manager::{DirectoryManager, DirectoryManagerFs},
    file_reader::{FileReader, FileReaderError},
    writer_file::FileWriter,
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{debug, error, trace};

use crate::{
    agent_control::{
        agent_id::AgentID,
        defaults::{FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA},
    },
    opamp::data_store::{OpAMPDataStore, StoreKey},
};

pub struct FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    directory_manager: D,
    file_rw: F,
    remote_dir: RwLock<RemoteDir>,
    local_dir: LocalDir,
}

pub struct LocalDir(PathBuf);

impl LocalDir {
    pub fn get_local_file_path(&self, agent_id: &AgentID, key: &StoreKey) -> PathBuf {
        self.0
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(agent_id)
            .join(build_config_name(key))
    }
}

pub struct RemoteDir(PathBuf);

impl RemoteDir {
    pub fn get_remote_file_path(&self, agent_id: &AgentID, key: &StoreKey) -> PathBuf {
        self.0
            .join(FOLDER_NAME_FLEET_DATA)
            .join(agent_id)
            .join(build_config_name(key))
    }
}

impl FileStore<LocalFile, DirectoryManagerFs> {
    pub fn new_local_fs(local_dir: PathBuf, remote_dir: PathBuf) -> Self {
        let remote_dir = RwLock::new(RemoteDir(remote_dir));
        let local_dir = LocalDir(local_dir);
        Self {
            file_rw: LocalFile,
            directory_manager: DirectoryManagerFs,
            local_dir,
            remote_dir,
        }
    }
}

impl<F, D> FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(file_rw: F, directory_manager: D, local_dir: PathBuf, remote_dir: PathBuf) -> Self {
        let remote_dir = RwLock::new(RemoteDir(remote_dir));
        let local_dir = LocalDir(local_dir);
        Self {
            file_rw,
            directory_manager,
            local_dir,
            remote_dir,
        }
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(&self, path: PathBuf) -> Result<Option<String>, FileReaderError> {
        let values_result = self.file_rw.read(path.as_path());
        match values_result {
            Ok(res) => Ok(Some(res)),
            Err(FileReaderError::FileNotFound(e)) => {
                trace!("file not found! {e}");
                // actively fallback to load local file
                Ok(None)
            }
            Err(err) => {
                error!("error loading file {}", path.display());
                Err(err)
            }
        }
    }

    /// ensures directory exists
    fn ensure_directory_existence(&self, values_file_path: &Path) -> Result<(), Error> {
        match values_file_path.parent() {
            None => Err(Error::other(format!(
                "cannot determine parent directory of path `{}`",
                values_file_path.display()
            ))),
            Some(parent) if !parent.exists() => {
                self.directory_manager.create(parent).map_err(Error::other)
            }
            Some(_) => Ok(()),
        }
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    fn get<T>(&self, key: PathBuf) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        self.load_file_if_present(key)
            // TODO: Address the generation of this error by
            // reworking the errors in the `fs` crate so they
            // emit std::io::Error instead.
            .map_err(Error::other)
            .and_then(|maybe_values| {
                maybe_values
                    .map(|s| serde_yaml::from_str(&s))
                    .transpose()
                    // TODO: Address the generation of this error by
                    // reworking the errors in the `fs` crate so they
                    // emit std::io::Error instead.
                    .map_err(|err| Error::new(ErrorKind::InvalidData, err))
            })
    }
}

impl<F, D> OpAMPDataStore for FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    type Error = io::Error;

    fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        let remote_dir = self.remote_dir.read().unwrap();
        self.get(remote_dir.get_remote_file_path(agent_id, key))
    }

    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        self.get(self.local_dir.get_local_file_path(agent_id, key))
    }

    fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &str, data: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        // I'm writing the locked file, not mutating the path
        // I think the OS will handle concurrent write/delete fine from all
        // threads/subprocesses of the program, but just in case. We can revisit later.
        #[allow(clippy::readonly_write_lock)]
        let remote_dir = self.remote_dir.write().unwrap();

        let remote_values_path = remote_dir.get_remote_file_path(agent_id, key);

        self.ensure_directory_existence(&remote_values_path)
            .map_err(|err| {
                Error::other(format!(
                    "error ensuring directory existence for {}: {}",
                    remote_values_path.display(),
                    err
                ))
            })?;
        let content =
            serde_yaml::to_string(data).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

        self.file_rw
            .write(remote_values_path.as_path(), content)
            .map_err(|err| {
                Error::other(format!(
                    "error writing file {}: {}",
                    remote_values_path.display(),
                    err
                ))
            })
    }

    fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error> {
        // I'm writing (deleting) the locked file, not mutating the path
        // I think the OS will handle concurrent write/delete fine from all
        // threads/subprocesses of the program, but just in case. We can revisit later.
        #[allow(clippy::readonly_write_lock)]
        let remote_dir = self.remote_dir.write().unwrap();

        let remote_path_file = remote_dir.get_remote_file_path(agent_id, key);
        if remote_path_file.exists() {
            debug!("deleting remote config: {:?}", remote_path_file);
            std::fs::remove_file(remote_path_file)?;
        }
        Ok(())
    }
}

pub fn build_config_name(name: &str) -> String {
    format!("{name}.yaml")
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    use assert_matches::assert_matches;
    use fs::{
        directory_manager::{
            DirectoryManagementError, DirectoryManager, mock::MockDirectoryManager,
        },
        file_reader::FileReader,
        mock::MockLocalFile,
        writer_file::FileWriter,
    };
    use rstest::{fixture, rstest};
    use serde_yaml::Value;

    use crate::{
        agent_control::{
            agent_id::AgentID,
            defaults::{
                INSTANCE_ID_FILENAME, STORE_KEY_INSTANCE_ID, STORE_KEY_LOCAL_DATA_CONFIG,
                STORE_KEY_OPAMP_DATA_CONFIG, default_capabilities,
            },
        },
        opamp::remote_config::hash::{ConfigState, Hash},
        values::{
            ConfigRepo,
            config::RemoteConfig,
            config_repository::{ConfigRepository, ConfigRepositoryError},
            yaml_config::YAMLConfig,
        },
    };

    use super::*;

    impl<F, S> FileStore<F, S>
    where
        S: DirectoryManager,
        F: FileWriter + FileReader,
    {
        pub fn get_testing_values_path(&self, agent_id: &AgentID, remote_enabled: bool) -> PathBuf {
            if remote_enabled {
                self.remote_dir
                    .read()
                    .unwrap()
                    .get_remote_file_path(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            } else {
                self.local_dir
                    .get_local_file_path(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            }
        }

        pub fn get_testing_instance_id_path(&self, agent_id: &AgentID) -> PathBuf {
            self.remote_dir
                .read()
                .unwrap()
                .get_remote_file_path(agent_id, STORE_KEY_INSTANCE_ID)
        }
    }

    impl From<PathBuf> for LocalDir {
        fn from(path: PathBuf) -> Self {
            Self(path)
        }
    }

    impl From<RemoteDir> for PathBuf {
        fn from(remote_dir: RemoteDir) -> Self {
            remote_dir.0
        }
    }

    impl From<PathBuf> for RemoteDir {
        fn from(path: PathBuf) -> Self {
            Self(path)
        }
    }

    impl From<LocalDir> for PathBuf {
        fn from(local_dir: LocalDir) -> Self {
            local_dir.0
        }
    }

    #[test]
    fn basic_get_uild_path() {
        let sa_dir = PathBuf::from("/super");
        let file_store = Arc::new(FileStore::new(
            MockLocalFile::default(),
            MockDirectoryManager::default(),
            PathBuf::default(),
            sa_dir.clone(),
        ));

        let agent_id = AgentID::try_from("test").unwrap();
        let path = file_store.get_testing_instance_id_path(&agent_id);
        assert_eq!(
            path,
            sa_dir
                .join(FOLDER_NAME_FLEET_DATA)
                .join("test")
                .join(INSTANCE_ID_FILENAME)
        );

        let agent_control_id = AgentID::AgentControl;
        let path = file_store.get_testing_instance_id_path(&agent_control_id);
        assert_eq!(
            path,
            sa_dir
                .join("fleet-data/agent-control")
                .join(INSTANCE_ID_FILENAME)
        );
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
            ConfigRepo::new(file_store).with_remote()
        } else {
            ConfigRepo::new(file_store)
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
        let repo = ConfigRepo::new(file_store).with_remote();

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
        let repo = ConfigRepo::new(file_store);

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
            ConfigRepo::new(file_store).with_remote()
        } else {
            ConfigRepo::new(file_store)
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

        let repo = ConfigRepo::new(file_store);

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
            DirectoryManagementError::ErrorCreatingDirectory(
                "dir name".to_string(),
                "oh now...".to_string(),
            ),
        );

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            local_dir_path.into(),
            remote_dir_path.into(),
        ));
        let repo = ConfigRepo::new(file_store);

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
        let repo = ConfigRepo::new(file_store);

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
        let repo = ConfigRepo::new(file_store);
        repo.delete_remote(&agent_id).unwrap();
    }
}
