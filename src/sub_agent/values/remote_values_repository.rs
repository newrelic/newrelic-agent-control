use crate::config::agent_values::AgentValues;
use crate::config::persister::directory_manager::{DirectoryManager, DirectoryManagerFs};
use crate::config::super_agent_configs::AgentID;
use crate::sub_agent::values::remote_values_repository::RemoteValuesRepositoryError::{
    DeleteError, ErrorCreatingRepository, ErrorDeletingRepository, StoreSerializeError,
    StoreWriteFileError,
};
use std::fs::Permissions;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::persister::config_writer_file::{Writer, WriterFile};
use crate::super_agent::defaults::{LOCAL_AGENT_DATA_DIR, REMOTE_AGENT_DATA_DIR};
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_family = "unix")]
pub(crate) const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Error, Debug)]
pub enum RemoteValuesRepositoryError {
    #[error("store error: `{0}`")]
    StoreWriteFileError(String),
    #[error("serialize error on store: `{0}`")]
    StoreSerializeError(String),
    #[error("error creating directory on store: `{0}`")]
    ErrorCreatingRepository(String),
    #[error("error deleting directory on store: `{0}`")]
    ErrorDeletingRepository(String),
    #[error("load error")]
    LoadError,
    #[error("cannot delete path `{0}`: `{1}`")]
    DeleteError(String, String),
}

pub trait RemoteValuesRepository {
    fn store(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), RemoteValuesRepositoryError>;

    fn delete_all(&self) -> Result<(), RemoteValuesRepositoryError>;

    fn delete_agent_values(&self, agent_id: &AgentID) -> Result<(), RemoteValuesRepositoryError>;
}

pub struct RemoteValuesRepositoryFile<S, F>
where
    S: DirectoryManager,
    F: Writer,
{
    directory_manager: S,
    writer: F,
    remote_conf_path: String,
    local_conf_path: String,
}

impl Default for RemoteValuesRepositoryFile<DirectoryManagerFs, WriterFile> {
    fn default() -> Self {
        RemoteValuesRepositoryFile {
            directory_manager: DirectoryManagerFs {},
            writer: WriterFile {},
            remote_conf_path: REMOTE_AGENT_DATA_DIR.to_string(),
            local_conf_path: LOCAL_AGENT_DATA_DIR.to_string(),
        }
    }
}

impl<S, F> RemoteValuesRepositoryFile<S, F>
where
    S: DirectoryManager,
    F: Writer,
{
    fn get_values_file_path(&self, agent_id: &AgentID, remote: bool) -> String {
        if remote {
            format!("{}/{}/values.yml", self.remote_conf_path, agent_id)
        } else {
            format!("{}/{}/values.yml", self.local_conf_path, agent_id)
        }
    }
}

impl<S, F> RemoteValuesRepository for RemoteValuesRepositoryFile<S, F>
where
    S: DirectoryManager,
    F: Writer,
{
    fn store(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), RemoteValuesRepositoryError> {
        // OpAMP protocol states that when only one config is present the key will be empty
        // https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#configuration-files

        let values_file_path = self.get_values_file_path(agent_id, true);

        //ensure directory exists and it's empty
        let mut values_dir_path = PathBuf::from(&values_file_path);
        values_dir_path.pop();

        self.directory_manager
            .delete(values_dir_path.as_path())
            .map_err(|e| ErrorDeletingRepository(e.to_string()))?;

        self.directory_manager
            .create(
                values_dir_path.as_path(),
                Permissions::from_mode(DIRECTORY_PERMISSIONS),
            )
            .map_err(|e| ErrorCreatingRepository(e.to_string()))?;

        let content =
            serde_yaml::to_string(agent_values).map_err(|e| StoreSerializeError(e.to_string()))?;

        self.writer
            .write(
                PathBuf::from(values_file_path.clone()).as_path(),
                content,
                Permissions::from_mode(FILE_PERMISSIONS),
            )
            .map_err(|e| StoreWriteFileError(e.to_string()))
    }

    fn delete_all(&self) -> Result<(), RemoteValuesRepositoryError> {
        let dest_path = Path::new(REMOTE_AGENT_DATA_DIR);
        self.directory_manager
            .delete(dest_path)
            .map_err(|e| DeleteError(REMOTE_AGENT_DATA_DIR.to_string(), e.to_string()))
    }

    fn delete_agent_values(&self, agent_id: &AgentID) -> Result<(), RemoteValuesRepositoryError> {
        let values_file_path = self.get_values_file_path(agent_id, true);
        //ensure directory exists
        let mut values_dir_path = PathBuf::from(values_file_path.clone());
        values_dir_path.pop();
        let values_dir = values_dir_path.to_str().unwrap().to_string();
        self.directory_manager
            .delete(values_dir_path.as_path())
            .map_err(|e| DeleteError(values_dir, e.to_string()))
    }
}

#[cfg(test)]
pub mod test {
    use crate::config::agent_type::agent_types::FinalAgent;
    use crate::config::agent_values::AgentValues;
    use crate::config::persister::config_persister::ConfigurationPersister;
    use crate::config::persister::config_writer_file::test::MockFileWriterMock;
    use crate::config::persister::config_writer_file::{Writer, WriterFile};
    use crate::config::persister::directory_manager::test::MockDirectoryManagerMock;
    use crate::config::persister::directory_manager::DirectoryManagementError::ErrorDeletingDirectory;
    use crate::config::persister::directory_manager::{DirectoryManager, DirectoryManagerFs};
    use crate::config::super_agent_configs::AgentID;
    use crate::sub_agent::values::remote_values_repository::{
        RemoteValuesRepository, RemoteValuesRepositoryError, RemoteValuesRepositoryFile,
    };
    use mockall::{mock, predicate};
    use std::fs;
    use std::fs::Permissions;
    use std::path::{Path, PathBuf};

    use crate::super_agent::defaults::LOCAL_AGENT_DATA_DIR;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;

    impl<S, F> RemoteValuesRepositoryFile<S, F>
    where
        S: DirectoryManager,
        F: Writer,
    {
        pub fn with_mocks(file_writer: F, directory_manager: S, remote_conf_path: &Path) -> Self {
            RemoteValuesRepositoryFile {
                writer: file_writer,
                directory_manager,
                remote_conf_path: remote_conf_path.to_str().unwrap().to_string(),
                local_conf_path: LOCAL_AGENT_DATA_DIR.to_string(),
            }
        }
    }

    mock! {
        pub(crate) RemoteValuesRepositoryMock {}

        impl RemoteValuesRepository for RemoteValuesRepositoryMock {
            fn store(
                &self,
                agent_id: &AgentID,
                agent_values: &AgentValues,
            ) -> Result<(), RemoteValuesRepositoryError> ;

            fn delete_all(&self) -> Result<(), RemoteValuesRepositoryError>;
            fn delete_agent_values(&self, agent_id: &AgentID) -> Result<(), RemoteValuesRepositoryError>;
        }
    }

    impl MockRemoteValuesRepositoryMock {
        pub fn should_store(&mut self, agent_id: &AgentID, agent_values: &AgentValues) {
            self.expect_store()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_values.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_delete_agent_values(&mut self, agent_id: &AgentID) {
            self.expect_delete_agent_values()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Ok(()));
        }
    }

    #[test]
    // This test is the only one that writes to an actual file in the FS
    fn test_configuration_persister_single_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut temp_path = PathBuf::from(&tempdir.path());
        temp_path.push("test_configuration_persister_single_file");

        let dir_manager = DirectoryManagerFs::default();
        let res = dir_manager.create(temp_path.as_path(), Permissions::from_mode(0o700));

        assert!(res.is_ok());
        let values_repo = RemoteValuesRepositoryFile::with_mocks(
            WriterFile::default(),
            DirectoryManagerFs::default(),
            temp_path.as_path(),
        );
        let agent_id = AgentID::new("SomeAgentID").unwrap();

        let mut agent_type: FinalAgent =
            serde_yaml::from_reader(AGENT_TYPE_SINGLE_FILE.as_bytes()).unwrap();
        let agent_values: AgentValues =
            serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();
        agent_type = agent_type.template_with(agent_values.clone()).unwrap();

        assert!(values_repo.store(&agent_id.clone(), &agent_values).is_ok());
        temp_path.push(agent_id);
        temp_path.push("values.yml");
        assert_eq!(
            AGENT_VALUES_SINGLE_FILE,
            fs::read_to_string(temp_path.as_path()).unwrap()
        );
    }

    #[test]
    fn test_error_deleting_directory() {
        let generated_conf_path = PathBuf::from("some/path");
        let file_writer = MockFileWriterMock::new();
        let mut directory_manager = MockDirectoryManagerMock::new();
        let agent_id = AgentID::new("SomeAgentID").unwrap();
        let mut agent_type: FinalAgent =
            serde_yaml::from_reader(AGENT_TYPE_SINGLE_FILE.as_bytes()).unwrap();
        let agent_values: AgentValues =
            serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();

        let mut agent_files_path = generated_conf_path.clone();
        agent_files_path.push(&agent_id);

        // populate agent type
        agent_type = agent_type.template_with(agent_values.clone()).unwrap();

        // Expectations
        directory_manager.should_not_delete(
            agent_files_path.as_path(),
            ErrorDeletingDirectory("oh now...".to_string()),
        );

        // Create persister
        let persister = RemoteValuesRepositoryFile::with_mocks(
            file_writer,
            directory_manager,
            generated_conf_path.as_path(),
        );

        let result = persister.store(&agent_id, &agent_values);
        assert!(result.is_err());
        assert_eq!(
            "error deleting directory on store: `cannot delete directory: `oh now...``".to_string(),
            result.err().unwrap().to_string()
        );
    }

    //////////////////////////////////////////////////
    // Fixtures
    //////////////////////////////////////////////////

    const AGENT_TYPE_SINGLE_FILE: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.1
variables:
  config_file:
    description: "Newrelic infra configuration path"
    type: file
    required: true
    file_path: newrelic-infra.yml
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${config_file}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 5
"#;

    const AGENT_VALUES_SINGLE_FILE: &str = r#"config_file: |
  license_key: 1234567890987654321
  log:
    level: debug
"#;
}
