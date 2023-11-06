use crate::config::persister::config_writer_file::{WriteError, Writer, WriterFile};
use crate::config::super_agent_configs::AgentID;
use thiserror::Error;

use serde::{Deserialize, Serialize};
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
use crate::file_reader::{FSFileReader, FileReader, FileReaderError};
use crate::super_agent::defaults::SUPER_AGENT_DATA_DIR;

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

pub struct HashRepositoryFile<R = FSFileReader, W = WriterFile>
where
    R: FileReader,
    W: Writer,
{
    file_reader: R,
    file_writer: W,
    conf_path: PathBuf,
}

impl HashRepositoryFile<FSFileReader, WriterFile> {
    // HashGetterPersisterFile with default writer and reader
    // and config path
    fn new(data_dir: String) -> Self {
        HashRepositoryFile {
            file_reader: FSFileReader,
            file_writer: WriterFile::default(),
            conf_path: PathBuf::from(data_dir),
        }
    }
}

impl Default for HashRepositoryFile<FSFileReader, WriterFile> {
    fn default() -> Self {
        HashRepositoryFile::new(SUPER_AGENT_DATA_DIR.to_string())
    }
}

impl<R, W> HashRepository for HashRepositoryFile<R, W>
where
    R: FileReader,
    W: Writer,
{
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError> {
        let mut conf_path = self.conf_path.clone();
        let hash_path = self.hash_file_path(agent_id, &mut conf_path);
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

impl<R, W> HashRepositoryFile<R, W>
where
    R: FileReader,
    W: Writer,
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
        Hash, HashRepository, HashRepositoryError, HashRepositoryFile, HASH_FILE_EXTENSION,
    };
    use crate::config::persister::config_persister_file::FILE_PERMISSIONS;
    use crate::config::persister::config_writer_file::test::MockFileWriterMock;
    use crate::config::persister::config_writer_file::Writer;
    use crate::config::super_agent_configs::AgentID;
    use crate::file_reader::test::MockFileReaderMock;
    use crate::file_reader::FileReader;
    use mockall::{mock, predicate};
    use std::fs::Permissions;
    use std::path::PathBuf;
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
        pub fn should_get_applied_hash(&mut self, agent_id: AgentID, hash: Hash) {
            self.expect_get()
                .with(predicate::eq(agent_id))
                .once()
                .returning(move |_| {
                    let mut hash = hash.clone();
                    hash.apply();
                    Ok(hash)
                });
        }
    }

    impl<R, W> HashRepositoryFile<R, W>
    where
        R: FileReader,
        W: Writer,
    {
        pub fn with_mocks(file_reader: R, file_writer: W, conf_path: PathBuf) -> Self {
            HashRepositoryFile {
                file_reader,
                file_writer,
                conf_path,
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

        let hash_repository =
            HashRepositoryFile::with_mocks(file_reader_mock, file_writer_mock, some_path);

        let result = hash_repository.save(&agent_id, &hash);
        assert!(result.is_ok());

        let result = hash_repository.get(&agent_id);
        assert_eq!(hash, result.unwrap());
    }
}
