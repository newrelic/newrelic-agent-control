use crate::config::agent_values::AgentValues;
use crate::config::persister::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use crate::config::super_agent_configs::AgentID;
use crate::sub_agent::values::values_repository::ValuesRepositoryError::DeleteError;
use std::fs::Permissions;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::persister::config_writer_file::WriteError;
use crate::file_reader::FileReaderError;
use crate::super_agent::defaults::{LOCAL_AGENT_DATA_DIR, REMOTE_AGENT_DATA_DIR, VALUES_FILENAME};
use log::error;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;

#[double]
use crate::config::persister::config_writer_file::WriterFile;
#[double]
use crate::file_reader::FSFileReader;
use mockall_double::double;

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
            #[allow(clippy::default_constructed_unit_structs)]
            file_reader: FSFileReader::default(),
        }
    }
}

impl ValuesRepositoryFile<DirectoryManagerFs> {
    pub fn with_remote(mut self) -> Self {
        self.remote_enabled = true;
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
            self.local_conf_path, agent_id, VALUES_FILENAME
        ))
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        PathBuf::from(format!(
            "{}/{}/{}",
            self.remote_conf_path, agent_id, VALUES_FILENAME
        ))
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(&self, path: PathBuf) -> Result<Option<String>, ValuesRepositoryError> {
        let remote_values_path = path.to_str().ok_or(ValuesRepositoryError::IncorrectPath)?;
        let values_result = self.file_reader.read(remote_values_path);
        match values_result {
            Err(FileReaderError::FileNotFound(_)) => {
                //actively fallback to load local file
                Ok(None)
            }
            Ok(res) => Ok(Some(res)),
            Err(err) => {
                // we log any unexpected error for now but maybe we should propagate it
                error!("error loading remote file {}", remote_values_path);
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
    use crate::config::agent_type::trivial_value::TrivialValue;
    use crate::config::agent_values::AgentValues;
    use crate::config::persister::directory_manager::test::MockDirectoryManagerMock;
    use crate::config::persister::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory,
    };
    use crate::config::persister::directory_manager::{DirectoryManager, DirectoryManagerFs};
    use crate::config::super_agent_configs::AgentID;
    use crate::sub_agent::values::values_repository::{
        ValuesRepository, ValuesRepositoryError, ValuesRepositoryFile,
    };
    use crate::super_agent::defaults::default_capabilities;
    use mockall::{mock, predicate};
    use std::collections::HashMap;
    use std::fs;
    use std::fs::Permissions;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    #[double]
    use crate::config::persister::config_writer_file::WriterFile;
    #[double]
    use crate::file_reader::FSFileReader;
    use mockall_double::double;

    impl<S> ValuesRepositoryFile<S>
    where
        S: DirectoryManager,
    {
        pub fn with_mocks(
            file_writer: WriterFile,
            directory_manager: S,
            file_reader: FSFileReader,
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
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_read(
            "some/remote/path/some_agent_id/values.yml".to_string(),
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

        assert_eq!(
            agent_values.get_from_normalized("some_config").unwrap(),
            TrivialValue::Bool(true)
        );
        assert_eq!(
            agent_values.get_from_normalized("another_item").unwrap(),
            TrivialValue::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_disabled() {
        //Mocks
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_read(
            "some/local/path/some_agent_id/values.yml".to_string(),
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

        assert_eq!(
            agent_values.get_from_normalized("some_config").unwrap(),
            TrivialValue::Bool(true)
        );
        assert_eq!(
            agent_values.get_from_normalized("another_item").unwrap(),
            TrivialValue::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_enabled_file_not_found_fallbacks_to_local() {
        //Mocks
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        let agent_values_content = "some_config: true\nanother_item: false";

        file_reader.should_not_read_file_not_found(
            "some/remote/path/some_agent_id/values.yml".to_string(),
            "some_error_message".to_string(),
        );

        file_reader.should_read(
            "some/local/path/some_agent_id/values.yml".to_string(),
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

        assert_eq!(
            agent_values.get_from_normalized("some_config").unwrap(),
            TrivialValue::Bool(true)
        );
        assert_eq!(
            agent_values.get_from_normalized("another_item").unwrap(),
            TrivialValue::Bool(false)
        );
    }

    #[test]
    fn test_load_local_file_not_found_should_return_defaults() {
        //Mocks
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader.should_not_read_file_not_found(
            "some/local/path/some_agent_id/values.yml".to_string(),
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
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader
            .should_not_read_io_error("some/remote/path/some_agent_id/values.yml".to_string());

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
        let mut file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let mut final_agent = FinalAgent::default();
        final_agent.set_capabilities(default_capabilities());

        file_reader
            .should_not_read_io_error("some/local/path/some_agent_id/values.yml".to_string());

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
        let file_reader = FSFileReader::default();
        let mut file_writer = WriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(HashMap::from([(
            "one_item".to_string(),
            TrivialValue::String("one value".to_string()),
        )]));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id"));
        dir_manager.should_create(
            Path::new("some/remote/path/some_agent_id"),
            Permissions::from_mode(0o700),
        );

        file_writer.should_write(
            Path::new("some/remote/path/some_agent_id/values.yml"),
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
        let file_reader = FSFileReader::default();
        let mut file_writer = WriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(HashMap::from([(
            "one_item".to_string(),
            TrivialValue::String("one value".to_string()),
        )]));

        dir_manager.should_not_delete(
            Path::new("some/remote/path/some_agent_id"),
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
        let file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(HashMap::from([(
            "one_item".to_string(),
            TrivialValue::String("one value".to_string()),
        )]));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id"));
        dir_manager.should_not_create(
            Path::new("some/remote/path/some_agent_id"),
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
        let file_reader = FSFileReader::default();
        let mut file_writer = WriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();
        let agent_values = AgentValues::new(HashMap::from([(
            "one_item".to_string(),
            TrivialValue::String("one value".to_string()),
        )]));

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id"));
        dir_manager.should_create(
            Path::new("some/remote/path/some_agent_id"),
            Permissions::from_mode(0o700),
        );

        file_writer.should_not_write(
            Path::new("some/remote/path/some_agent_id/values.yml"),
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
        let file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
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
        let file_reader = FSFileReader::default();
        let file_writer = WriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some_agent_id").unwrap();

        dir_manager.should_delete(Path::new("some/remote/path/some_agent_id"));

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

    // This test is the only one that writes to an actual file in the FS
    // #[test]
    // fn test_store_remote_no_mocks() {
    //     let tempdir = tempfile::tempdir().unwrap();
    //
    //     let mut local_dir = PathBuf::from(&tempdir.path());
    //     local_dir.push("local_dir");
    //
    //     let mut remote_dir = PathBuf::from(&tempdir.path());
    //     remote_dir.push("remote_dir");
    //
    //     let file_reader = FSFileReader::default();
    //     let dir_manager = DirectoryManagerFs::default();
    //     let remote_enabled = true;
    //
    //     // Ensure dir exists
    //     let res = dir_manager.create(remote_dir.as_path(), Permissions::from_mode(0o700));
    //     assert!(res.is_ok());
    //
    //     let values_repo = ValuesRepositoryFile::with_mocks(
    //         WriterFile::default(),
    //         DirectoryManagerFs::default(),
    //         file_reader,
    //         local_dir.as_path(),
    //         remote_dir.as_path(),
    //         remote_enabled,
    //     );
    //     let agent_id = AgentID::new("SomeAgentID").unwrap();
    //
    //     let agent_values: AgentValues =
    //         serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();
    //
    //     values_repo
    //         .store_remote(&agent_id.clone(), &agent_values)
    //         .unwrap();
    //
    //     remote_dir.push(agent_id);
    //     remote_dir.push("values.yml");
    //
    //     assert_eq!(
    //         AGENT_VALUES_SINGLE_FILE,
    //         fs::read_to_string(remote_dir.as_path()).unwrap()
    //     );
    // }

    //////////////////////////////////////////////////
    // Fixtures
    //////////////////////////////////////////////////
    const AGENT_VALUES_SINGLE_FILE: &str = r#"config_file: |
  license_key: 1234567890987654321
  log:
    level: debug
"#;
}
