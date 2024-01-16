use crate::config::agent_values::AgentValues;
use crate::config::super_agent_configs::AgentID;
use crate::fs::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use crate::sub_agent::values::values_repository::ValuesRepositoryError::DeleteError;
use std::fs::Permissions;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::agent_type::agent_types::FinalAgent;
#[cfg_attr(test, mockall_double::double)]
use crate::fs::file_reader::FSFileReader;
use crate::fs::file_reader::FileReaderError;
use crate::fs::writer_file::WriteError;
#[cfg_attr(test, mockall_double::double)]
use crate::fs::writer_file::WriterFile;
use crate::super_agent::defaults::{LOCAL_AGENT_DATA_DIR, REMOTE_AGENT_DATA_DIR, VALUES_PATH};
use log::error;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_family = "unix")]
pub(crate) const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Error, Debug)]
pub enum ValuesRepositoryError {
    #[error("serialize error on store: `{0}`")]
    StoreSerializeError(#[from] serde_yaml::Error),
    #[error("incorrect path")]
    IncorrectPath,
    #[error("cannot delete path `{0}`: `{1}`")]
    DeleteError(String, String),
    #[error("directory manager error: `{0}`")]
    DirectoryManagementError(#[from] DirectoryManagementError),
    #[error("file write error: `{0}`")]
    WriteError(#[from] WriteError),
    #[error("file read error: `{0}`")]
    ReadError(#[from] FileReaderError),
}

pub trait ValuesRepository {
    fn load(
        &self,
        agent_id: &AgentID,
        final_agent: &FinalAgent,
    ) -> Result<AgentValues, ValuesRepositoryError>;

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), ValuesRepositoryError>;

    fn delete_remote_all(&self) -> Result<(), ValuesRepositoryError>;

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;
}

pub struct ValuesRepositoryFile<S>
where
    S: DirectoryManager,
{
    directory_manager: S,
    writer: WriterFile,
    remote_conf_path: String,
    local_conf_path: String,
    remote_enabled: bool,
    file_reader: FSFileReader,
}

impl Default for ValuesRepositoryFile<DirectoryManagerFs> {
    fn default() -> Self {
        ValuesRepositoryFile {
            directory_manager: DirectoryManagerFs {},
            writer: WriterFile::default(),
            remote_conf_path: REMOTE_AGENT_DATA_DIR.to_string(),
            local_conf_path: LOCAL_AGENT_DATA_DIR.to_string(),
            remote_enabled: false,
            file_reader: FSFileReader::default(),
        }
    }
}

impl ValuesRepositoryFile<DirectoryManagerFs> {
    pub fn with_remote(mut self) -> Self {
        self.remote_enabled = true;
        self
    }

    // Change remote conf path for integration tests
    // TODO : move this under a feature
    pub fn with_remote_conf_path(mut self, path: String) -> Self {
        self.remote_conf_path = path;
        self
    }

    // Change remote conf path for integration tests
    // TODO : move this under a feature
    pub fn with_local_conf_path(mut self, path: String) -> Self {
        self.local_conf_path = path;
        self
    }
}

impl<S> ValuesRepositoryFile<S>
where
    S: DirectoryManager,
{
    pub fn get_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        PathBuf::from(format!(
            "{}/{}/{}",
            self.local_conf_path, agent_id, VALUES_PATH
        ))
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        // This file (soon files) will be removed often, but its parent directory contains files
        // that should persist across these deletions. As opposed to its non-remote counterpart in
        // `get_values_file_path`, we put the values file inside its own directory, which will
        // be recreated each time a remote config is received, leaving the other files untouched.
        PathBuf::from(format!(
            "{}/{}/{}",
            self.remote_conf_path, agent_id, VALUES_PATH
        ))
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(&self, path: PathBuf) -> Result<Option<String>, ValuesRepositoryError> {
        let values_result = self.file_reader.read(path.as_path());
        match values_result {
            Err(FileReaderError::FileNotFound(_)) => {
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
}

impl<S> ValuesRepository for ValuesRepositoryFile<S>
where
    S: DirectoryManager,
{
    fn load(
        &self,
        agent_id: &AgentID,
        agent_type: &FinalAgent,
    ) -> Result<AgentValues, ValuesRepositoryError> {
        let mut values_result: Option<String> = None;

        if self.remote_enabled && agent_type.has_remote_management() {
            let remote_values_path = self.get_remote_values_file_path(agent_id);
            values_result = self.load_file_if_present(remote_values_path)?;
        }

        if values_result.is_none() {
            let local_values_path = self.get_values_file_path(agent_id);
            values_result = self.load_file_if_present(local_values_path)?;
        }

        if let Some(contents) = values_result {
            Ok(serde_yaml::from_str(&contents)?)
        } else {
            Ok(AgentValues::default())
        }
    }

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), ValuesRepositoryError> {
        // OpAMP protocol states that when only one config is present the key will be empty
        // https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#configuration-files

        let values_file_path = self.get_remote_values_file_path(agent_id);

        //ensure directory exists and it's empty
        let mut values_dir_path = PathBuf::from(&values_file_path);
        values_dir_path.pop();

        self.directory_manager.delete(values_dir_path.as_path())?;
        self.directory_manager.create(
            values_dir_path.as_path(),
            Permissions::from_mode(DIRECTORY_PERMISSIONS),
        )?;

        let content = serde_yaml::to_string(agent_values)?;

        Ok(self.writer.write(
            values_file_path.clone().as_path(),
            content,
            Permissions::from_mode(FILE_PERMISSIONS),
        )?)
    }

    fn delete_remote_all(&self) -> Result<(), ValuesRepositoryError> {
        let dest_path = Path::new(self.remote_conf_path.as_str());
        self.directory_manager
            .delete(dest_path)
            .map_err(|e| DeleteError(self.remote_conf_path.to_string(), e.to_string()))
    }

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError> {
        let values_file_path = self.get_remote_values_file_path(agent_id);
        //ensure directory exists
        let mut values_dir_path = values_file_path.clone();
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
    use crate::config::super_agent_configs::AgentID;
    use crate::fs::directory_manager::test::MockDirectoryManagerMock;
    use crate::fs::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory,
    };
    use crate::fs::directory_manager::DirectoryManager;
    use crate::fs::writer_file::MockWriterFile;
    use crate::sub_agent::values::values_repository::{
        ValuesRepository, ValuesRepositoryError, ValuesRepositoryFile,
    };
    use mockall::{mock, predicate};
    use serde_yaml::{Mapping, Value};
    use std::collections::HashMap;
    use std::fs::Permissions;
    use std::path::Path;

    use crate::config::agent_type::trivial_value::TrivialValue;
    use crate::fs::file_reader::MockFSFileReader;
    use crate::super_agent::defaults::default_capabilities;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;

    impl<S> ValuesRepositoryFile<S>
    where
        S: DirectoryManager,
    {
        pub fn with_mocks(
            file_writer: MockWriterFile,
            directory_manager: S,
            file_reader: MockFSFileReader,
            local_conf_path: &Path,
            remote_conf_path: &Path,
            remote_enabled: bool,
        ) -> Self {
            ValuesRepositoryFile {
                writer: file_writer,
                directory_manager,
                remote_conf_path: remote_conf_path.to_str().unwrap().to_string(),
                local_conf_path: local_conf_path.to_str().unwrap().to_string(),
                file_reader,
                remote_enabled,
            }
        }
    }

    mock! {
        pub(crate) RemoteValuesRepositoryMock {}

        impl ValuesRepository for RemoteValuesRepositoryMock {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                agent_values: &AgentValues,
            ) -> Result<(), ValuesRepositoryError> ;
             fn load(
                &self,
                agent_id: &AgentID,
                final_agent: &FinalAgent,
            ) -> Result<AgentValues, ValuesRepositoryError>;
            fn delete_remote_all(&self) -> Result<(), ValuesRepositoryError>;
            fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;
        }
    }

    impl MockRemoteValuesRepositoryMock {
        pub fn should_load(
            &mut self,
            agent_id: &AgentID,
            final_agent: &FinalAgent,
            agent_values: &AgentValues,
        ) {
            let agent_values = agent_values.clone();
            self.expect_load()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(move |_, _| Ok(agent_values.clone()));
        }

        pub fn should_not_load(&mut self, agent_id: &AgentID, final_agent: &FinalAgent) {
            self.expect_load()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(move |_, _| Err(ValuesRepositoryError::IncorrectPath));
        }

        pub fn should_store_remote(&mut self, agent_id: &AgentID, agent_values: &AgentValues) {
            self.expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_values.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_delete_remote(&mut self, agent_id: &AgentID) {
            self.expect_delete_remote()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Ok(()));
        }

        pub fn should_not_delete_remote(&mut self, agent_id: &AgentID) {
            self.expect_delete_remote()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Err(ValuesRepositoryError::IncorrectPath));
        }
    }

    #[test]
    fn test_load_when_remote_enabled() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_read(
            Path::new("some/remote/path/some_agent_id/values/values.yaml"),
            agent_values_content.to_string(),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let agent_values = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(agent_values.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            agent_values.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_disabled() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_read(
            Path::new("some/local/path/some_agent_id/values/values.yaml"),
            agent_values_content.to_string(),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let agent_values = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(agent_values.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            agent_values.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_enabled_file_not_found_fallbacks_to_local() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_not_read_file_not_found(
            Path::new("some/remote/path/some_agent_id/values/values.yaml"),
            "some_error_message".to_string(),
        );

        file_reader.should_read(
            Path::new("some/local/path/some_agent_id/values/values.yaml"),
            agent_values_content.to_string(),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let agent_values = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(agent_values.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            agent_values.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_local_file_not_found_should_return_defaults() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader.should_not_read_file_not_found(
            Path::new("some/local/path/some_agent_id/values/values.yaml"),
            "some message".to_string(),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let agent_values = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(agent_values, AgentValues::default());
    }

    #[test]
    fn test_load_when_remote_enabled_io_error() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader.should_not_read_io_error(Path::new(
            "some/remote/path/some_agent_id/values/values.yaml",
        ));

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.load(&agent_id, &final_agent);

        assert!(result.is_err());
        assert_eq!(
            "file read error: `error reading contents: `permission denied``".to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_load_local_io_error() {
        //Mocks
        let mut file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader.should_not_read_io_error(Path::new(
            "some/local/path/some_agent_id/values/values.yaml",
        ));

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.load(&agent_id, &final_agent);

        assert!(result.is_err());
        assert_eq!(
            "file read error: `error reading contents: `permission denied``".to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_store_remote() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let mut file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(Value::Mapping(Mapping::from_iter([(
            "one_item".into(),
            "one value".into(),
        )])));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id/values"));
        dir_manager.should_create(
            Path::new("some/remote/path/some_agent_id/values"),
            Permissions::from_mode(0o700),
        );

        file_writer.should_write(
            Path::new("some/remote/path/some_agent_id/values/values.yaml"),
            "one_item: one value\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        repo.store_remote(&agent_id, &agent_values).unwrap();
    }

    #[test]
    fn test_store_remote_error_deleting_dir() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(Value::Mapping(Mapping::from_iter([(
            "one_item".into(),
            "one value".into(),
        )])));

        dir_manager.should_not_delete(
            Path::new("some/remote/path/some_agent_id/values"),
            ErrorDeletingDirectory("oh now...".to_string()),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &agent_values);
        assert!(result.is_err());
        assert_eq!(
            "directory manager error: `cannot delete directory: `oh now...``".to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_store_remote_error_creating_dir() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(Value::Mapping(Mapping::from_iter([(
            "one_item".into(),
            "one value".into(),
        )])));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id/values"));
        dir_manager.should_not_create(
            Path::new("some/remote/path/some_agent_id/values"),
            Permissions::from_mode(0o700),
            ErrorCreatingDirectory("dir name".to_string(), "oh now...".to_string()),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &agent_values);
        assert!(result.is_err());
        assert_eq!(
            "directory manager error: `cannot create directory `dir name` : `oh now...``"
                .to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_store_remote_error_writing_file() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let mut file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(Value::Mapping(Mapping::from_iter([(
            "one_item".into(),
            "one value".into(),
        )])));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id/values"));
        dir_manager.should_create(
            Path::new("some/remote/path/some_agent_id/values"),
            Permissions::from_mode(0o700),
        );

        file_writer.should_not_write(
            Path::new("some/remote/path/some_agent_id/values/values.yaml"),
            "one_item: one value\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &agent_values);

        assert!(result.is_err());
        assert_eq!(
            "file write error: `error creating file: `permission denied``".to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_delete_remote_all() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        dir_manager.should_delete(Path::new("some/remote/path"));

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        repo.delete_remote_all().unwrap();
    }

    #[test]
    fn test_delete_remote() {
        //Mocks
        let file_reader = MockFSFileReader::default();
        let file_writer = MockWriterFile::new();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id/values"));

        let repo = ValuesRepositoryFile::with_mocks(
            file_writer,
            dir_manager,
            file_reader,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        repo.delete_remote(&agent_id).unwrap();
    }
}
