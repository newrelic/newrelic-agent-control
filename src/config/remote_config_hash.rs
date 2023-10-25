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
}

impl Hash {
    pub fn new(hash: String) -> Self {
        Self { hash, applied: false }
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
    fn save(&self, agent_id: AgentID, hash: Hash) -> Result<(), HashRepositoryError>;
    fn get(&self, agent_id: AgentID) -> Result<Hash, HashRepositoryError>;
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
    pub fn new(data_dir: String) -> Self {
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
    fn save(&self, agent_id: AgentID, hash: Hash) -> Result<(), HashRepositoryError> {
        let mut hash_file_path = self.conf_path.clone();
        let hash_file = format!("{}.{}", agent_id.0.as_str(), HASH_FILE_EXTENSION);
        hash_file_path.push(hash_file);
        let writing_result = self.write(hash_file_path.as_path(), serde_yaml::to_string(&hash)?);
        Ok(writing_result?)
    }

    fn get(&self, agent_id: AgentID) -> Result<Hash, HashRepositoryError> {
        let mut hash_file_path = self.conf_path.clone();
        let hash_file = format!("{}.{}", agent_id.0.as_str(), HASH_FILE_EXTENSION);
        hash_file_path.push(hash_file);
        let contents = self.file_reader.read(hash_file_path.to_str().unwrap())?;
        let result = serde_yaml::from_str(&contents);
        Ok(result?)
    }
}

impl<R, W> HashRepositoryFile<R, W>
where
    R: FileReader,
    W: Writer,
{
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
    use crate::config::remote_config_hash::{Hash, HashRepository, HashRepositoryError};
    use crate::config::super_agent_configs::AgentID;
    use mockall::mock;
    ////////////////////////////////////////////////////////////////////////////////////
    // Mock
    ////////////////////////////////////////////////////////////////////////////////////
    mock! {
        pub(crate) HashRepositoryMock {}

        impl HashRepository for HashRepositoryMock {

            fn save(&self, agent_id: AgentID, hash:Hash) -> Result<(), HashRepositoryError>;

            fn get(&self, agent_id: AgentID) -> Result<Hash, HashRepositoryError>;
        }
    }
}
