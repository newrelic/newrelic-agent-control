use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
use crate::config::persister::config_writer_file::WriteError;
#[cfg_attr(test, mockall_double::double)]
use crate::config::persister::config_writer_file::WriterFile;
use crate::config::persister::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use crate::config::super_agent_configs::AgentID;
#[cfg_attr(test, mockall_double::double)]
use crate::file_reader::FSFileReader;
use crate::file_reader::FileReaderError;
use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_DATA_DIR};
use serde::{Deserialize, Serialize};
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::debug;

#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "state")]
enum ConfigState {
    Applying,
    Applied,
    Failed { error_message: String },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct Hash {
    hash: String,
    #[serde(flatten)]
    state: ConfigState,
}

#[derive(Error, Debug)]
pub enum HashRepositoryError {
    #[error("file error: `{0}`")]
    SaveError(#[from] WriteError),

    #[error("serde error: `{0}`")]
    SerdeError(#[from] serde_yaml::Error),

    #[error("file reader error: `{0}`")]
    FileRead(#[from] FileReaderError),

    #[error("hash directory create error: `{0}`")]
    DirectoryCreateError(#[from] DirectoryManagementError),

    #[error("no path found")]
    WrongPath,
}

impl Hash {
    pub fn get(&self) -> String {
        self.hash.clone()
    }
    pub fn is_applied(&self) -> bool {
        self.state == ConfigState::Applied
    }

    pub fn is_applying(&self) -> bool {
        self.state == ConfigState::Applying
    }

    pub fn is_failed(&self) -> bool {
        // if let self.state = ConfigState::Failed(msg)
        matches!(&self.state, ConfigState::Failed { .. })
    }

    pub fn error_message(&self) -> Option<String> {
        match &self.state {
            ConfigState::Failed { error_message: msg } => Some(msg.clone()),
            _ => None,
        }
    }
}

impl Hash {
    pub fn new(hash: String) -> Self {
        Self {
            hash,
            state: ConfigState::Applying,
        }
    }
    pub fn apply(&mut self) {
        self.state = ConfigState::Applied;
    }

    // It is mandatory for a failed hash to have the error
    pub fn fail(&mut self, error_message: String) {
        self.state = ConfigState::Failed { error_message };
    }
}

pub trait HashRepository {
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError>;
    fn get(&self, agent_id: &AgentID) -> Result<Hash, HashRepositoryError>;
}

const HASH_FILE_EXTENSION: &str = "yaml";

pub struct HashRepositoryFile<D = DirectoryManagerFs>
where
    D: DirectoryManager,
{
    file_reader: FSFileReader,
    file_writer: WriterFile,
    conf_path: PathBuf,
    directory_manager: D,
}

impl HashRepositoryFile<DirectoryManagerFs> {
    // HashGetterPersisterFile with default writer and reader
    // and config path
    fn new(data_dir: String) -> Self {
        HashRepositoryFile {
            file_reader: FSFileReader::default(),
            file_writer: WriterFile::default(),
            conf_path: PathBuf::from(data_dir),
            directory_manager: DirectoryManagerFs::default(),
        }
    }
}

impl Default for HashRepositoryFile {
    fn default() -> Self {
        HashRepositoryFile::new(SUPER_AGENT_DATA_DIR.to_string())
    }
}

impl<D> HashRepository for HashRepositoryFile<D>
where
    D: DirectoryManager,
{
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError> {
        let mut conf_path = self.conf_path.clone();
        let hash_path = self.hash_file_path(agent_id, &mut conf_path);
        // Ensure the directory exists
        let mut hash_dir = PathBuf::from(hash_path);
        hash_dir.pop();
        self.directory_manager.create(
            hash_dir.as_path(),
            Permissions::from_mode(DIRECTORY_PERMISSIONS),
        )?;

        let writing_result = self.write(hash_path, serde_yaml::to_string(hash)?);
        Ok(writing_result?)
    }

    fn get(&self, agent_id: &AgentID) -> Result<Hash, HashRepositoryError> {
        let mut conf_path = self.conf_path.clone();
        let hash_path = self.hash_file_path(agent_id, &mut conf_path);
        debug!("Reading hash file at {}", hash_path.to_string_lossy());
        let contents = self.file_reader.read(hash_path)?;
        let result = serde_yaml::from_str(&contents);
        Ok(result?)
    }
}

impl HashRepositoryFile {
    pub fn new_sub_agent_repository() -> Self {
        HashRepositoryFile::new(REMOTE_AGENT_DATA_DIR.to_string())
    }
}

impl<D> HashRepositoryFile<D>
where
    D: DirectoryManager,
{
    fn hash_file_path<'a>(&'a self, agent_id: &AgentID, path: &'a mut PathBuf) -> &Path {
        let hash_file = format!("{}.{}", agent_id.get(), HASH_FILE_EXTENSION);
        path.push(hash_file);
        path
    }

    // Wrapper for linux with unix specific permissions
    #[cfg(target_family = "unix")]
    fn write(&self, path: &Path, content: String) -> Result<(), WriteError> {
        self.file_writer
            .write(path, content, Permissions::from_mode(FILE_PERMISSIONS))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod test {
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;

    use super::{
        ConfigState, Hash, HashRepository, HashRepositoryError, HashRepositoryFile,
        DIRECTORY_PERMISSIONS, HASH_FILE_EXTENSION,
    };
    use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
    use crate::config::persister::config_writer_file::MockWriterFile;
    use crate::config::persister::directory_manager::test::MockDirectoryManagerMock;
    use crate::config::persister::directory_manager::DirectoryManager;
    use crate::config::super_agent_configs::AgentID;
    use crate::file_reader::MockFSFileReader;
    use mockall::{mock, predicate};
    use std::fs::Permissions;
    use std::path::PathBuf;

    impl Hash {
        pub fn applied(hash: String) -> Self {
            Self {
                hash,
                state: ConfigState::Applied,
            }
        }

        pub fn failed(hash: String, error_message: String) -> Self {
            Self {
                hash,
                state: ConfigState::Failed { error_message },
            }
        }
    }
    ////////////////////////////////////////////////////////////////////////////////////
    // Mock
    ////////////////////////////////////////////////////////////////////////////////////
    mock! {
        pub(crate) HashRepositoryMock {}

        impl HashRepository for HashRepositoryMock {

            fn save(&self, agent_id: &AgentID, hash:&Hash) -> Result<(), HashRepositoryError>;

            fn get(&self, agent_id: &AgentID) -> Result<Hash, HashRepositoryError>;
        }
    }

    impl MockHashRepositoryMock {
        pub fn should_get_hash(&mut self, agent_id: &AgentID, hash: Hash) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .returning(move |_| Ok(hash.clone()));
        }
        pub fn should_save_hash(&mut self, agent_id: &AgentID, hash: &Hash) {
            self.expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Ok(()));
        }
    }

    impl<D> HashRepositoryFile<D>
    where
        D: DirectoryManager,
    {
        pub fn with_mocks(
            file_reader: MockFSFileReader,
            file_writer: MockWriterFile,
            directory_manager: D,
            conf_path: PathBuf,
        ) -> Self {
            HashRepositoryFile {
                file_reader,
                file_writer,
                conf_path,
                directory_manager,
            }
        }
    }

    #[test]
    fn test_save_and_get_hash() {
        let some_path = PathBuf::from("some/path");
        let mut file_writer_mock = MockWriterFile::new();
        let mut file_reader_mock = MockFSFileReader::default();
        let file_permissions = Permissions::from_mode(FILE_PERMISSIONS);
        let agent_id = AgentID::new("SomeAgentID").unwrap();
        let mut hash = Hash::new("123456789".to_string());
        hash.apply();

        // This indentation and the single quotes is to match serde yaml
        let content = r#"hash: '123456789'
state: applied
"#;

        let mut expected_path = some_path.clone();
        expected_path.push(format!("{}.{}", agent_id.get(), HASH_FILE_EXTENSION));

        file_reader_mock.should_read(expected_path.as_path(), content.to_string());
        file_writer_mock.should_write(
            expected_path.as_path(),
            content.to_string(),
            file_permissions,
        );

        let mut expected_path_dir = expected_path.clone();
        expected_path_dir.pop();
        let mut dir_manager = MockDirectoryManagerMock::new();
        dir_manager.should_create(
            expected_path_dir.as_path(),
            Permissions::from_mode(DIRECTORY_PERMISSIONS),
        );

        let hash_repository = HashRepositoryFile::with_mocks(
            file_reader_mock,
            file_writer_mock,
            dir_manager,
            some_path,
        );

        let result = hash_repository.save(&agent_id, &hash);
        assert!(result.is_ok());

        let result = hash_repository.get(&agent_id);
        assert_eq!(hash, result.unwrap());
    }

    #[test]
    fn test_config_state_default_status() {
        //default status for a hash should be applying
        let hash = Hash::new("some-hash".into());
        assert!(hash.is_applying())
    }

    #[test]
    fn test_config_state_transition() {
        // hash can change state. This is not ideal, as an applied hash should not go to failed
        let mut hash = Hash::new("some-hash".into());
        assert!(hash.is_applying());
        hash.apply();
        assert!(hash.is_applied());
        hash.fail("this is an error message".to_string());
        assert!(hash.is_failed());
    }

    #[test]
    fn test_hash_serialization() {
        let mut hash = Hash::new("123456789".to_string());
        let expected = "hash: '123456789'\nstate: applying\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());

        hash.apply();
        let expected = "hash: '123456789'\nstate: applied\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());

        hash.fail("this is an error message".to_string());
        let expected =
            "hash: '123456789'\nstate: failed\nerror_message: this is an error message\n";
        assert_eq!(expected, serde_yaml::to_string(&hash).unwrap());
    }

    #[test]
    fn test_hash_deserialization() {
        let mut hash = Hash::new("123456789".to_string());
        let content = "hash: '123456789'\nstate: applying\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());

        hash.apply();
        let content = "hash: '123456789'\nstate: applied\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());

        hash.fail("this is an error message".to_string());
        let content = "hash: '123456789'\nstate: failed\nerror_message: this is an error message\n";
        assert_eq!(hash, serde_yaml::from_str::<Hash>(content).unwrap());
    }
}
