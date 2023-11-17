use crate::config::persister::config_writer_file::{WriteError, Writer, WriterFile};
use crate::config::super_agent_configs::AgentID;
use thiserror::Error;

use serde::{Deserialize, Serialize};
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
use crate::config::persister::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use crate::file_reader::{FSFileReader, FileReader, FileReaderError};
use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_DATA_DIR};

#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct Hash {
    hash: String,
    applied: bool,
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
    pub fn new(hash: String) -> Self {
        Self {
            hash,
            applied: false,
        }
    }
    pub fn get(&self) -> String {
        self.hash.clone()
    }

    pub fn is_applied(&self) -> bool {
        self.applied
    }

    pub fn apply(&mut self) {
        self.applied = true
    }
}

pub trait HashRepository {
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError>;
    fn get(&self, agent_id: &AgentID) -> Result<Hash, HashRepositoryError>;
}

const HASH_FILE_EXTENSION: &str = "yaml";

pub struct HashRepositoryFile<R = FSFileReader, W = WriterFile, D = DirectoryManagerFs>
where
    R: FileReader,
    W: Writer,
    D: DirectoryManager,
{
    file_reader: R,
    file_writer: W,
    conf_path: PathBuf,
    directory_manager: D,
}

impl HashRepositoryFile<FSFileReader, WriterFile, DirectoryManagerFs> {
    // HashGetterPersisterFile with default writer and reader
    // and config path
    fn new(data_dir: String) -> Self {
        HashRepositoryFile {
            file_reader: FSFileReader,
            file_writer: WriterFile::default(),
            conf_path: PathBuf::from(data_dir),
            directory_manager: DirectoryManagerFs::default(),
        }
    }
}

impl Default for HashRepositoryFile<FSFileReader, WriterFile> {
    fn default() -> Self {
        HashRepositoryFile::new(SUPER_AGENT_DATA_DIR.to_string())
    }
}

impl<R, W, D> HashRepository for HashRepositoryFile<R, W, D>
where
    R: FileReader,
    W: Writer,
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
        let hash_path = self.hash_file_path(agent_id, &mut conf_path).to_str();
        if let Some(path) = hash_path {
            let contents = self.file_reader.read(path)?;
            let result = serde_yaml::from_str(&contents);
            return Ok(result?);
        }
        Err(HashRepositoryError::WrongPath)
    }
}

impl HashRepositoryFile<FSFileReader, WriterFile> {
    pub fn new_sub_agent_repository() -> Self {
        HashRepositoryFile::new(REMOTE_AGENT_DATA_DIR.to_string())
    }
}

impl<R, W, D> HashRepositoryFile<R, W, D>
where
    R: FileReader,
    W: Writer,
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
        Hash, HashRepository, HashRepositoryError, HashRepositoryFile, DIRECTORY_PERMISSIONS,
        HASH_FILE_EXTENSION,
    };
    use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
    use crate::config::persister::config_writer_file::test::MockFileWriterMock;
    use crate::config::persister::config_writer_file::Writer;
    use crate::config::persister::directory_manager::test::MockDirectoryManagerMock;
    use crate::config::persister::directory_manager::DirectoryManager;
    use crate::config::super_agent_configs::AgentID;
    use crate::file_reader::test::MockFileReaderMock;
    use crate::file_reader::FileReader;
    use mockall::{mock, predicate};
    use std::fs::Permissions;
    use std::path::PathBuf;

    impl Hash {
        pub fn applied(hash: String) -> Self {
            Self {
                applied: true,
                hash,
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
        pub fn should_get_applied_hash(&mut self, agent_id: &AgentID, hash: Hash) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .returning(move |_| {
                    let mut hash = hash.clone();
                    hash.apply();
                    Ok(hash)
                });
        }
        pub fn should_save_hash(&mut self, agent_id: &AgentID, hash: &Hash) {
            self.expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Ok(()));
        }
    }

    impl<R, W, D> HashRepositoryFile<R, W, D>
    where
        R: FileReader,
        W: Writer,
        D: DirectoryManager,
    {
        pub fn with_mocks(
            file_reader: R,
            file_writer: W,
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
        let mut file_writer_mock = MockFileWriterMock::new();
        let mut file_reader_mock = MockFileReaderMock::new();
        let file_permissions = Permissions::from_mode(FILE_PERMISSIONS);
        let agent_id = AgentID::new("SomeAgentID").unwrap();
        let mut hash = Hash::new("123456789".to_string());
        hash.apply();

        // This indentation and the single quotes is to match serde yaml
        let content = r#"hash: '123456789'
applied: true
"#;

        let mut expected_path = some_path.clone();
        expected_path.push(format!("{}.{}", agent_id.get(), HASH_FILE_EXTENSION));

        file_reader_mock.should_read(
            expected_path.to_str().unwrap().to_string(),
            content.to_string(),
        );
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
}
