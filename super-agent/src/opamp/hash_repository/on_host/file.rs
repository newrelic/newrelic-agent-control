use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_DATA_DIR};
use fs::directory_manager::DirectoryManagementError;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use fs::file_reader::FileReader;
use fs::file_reader::FileReaderError;
use fs::writer_file::{FileWriter, WriteError};
use fs::LocalFile;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::debug;

#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

const HASH_FILE_NAME: &str = "hash.yaml";

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

pub struct HashRepositoryFile<F = LocalFile, D = DirectoryManagerFs>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_rw: F,
    conf_path: PathBuf,
    directory_manager: D,
}

impl HashRepositoryFile<LocalFile, DirectoryManagerFs> {
    // HashGetterPersisterFile with default writer and reader
    // and config path
    fn new(data_dir: String) -> Self {
        HashRepositoryFile {
            file_rw: LocalFile,
            conf_path: PathBuf::from(data_dir),
            directory_manager: DirectoryManagerFs::default(),
        }
    }
}

impl Default for HashRepositoryFile {
    fn default() -> Self {
        HashRepositoryFile::new(SUPER_AGENT_DATA_DIR().to_string())
    }
}

impl<F, D> HashRepository for HashRepositoryFile<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
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

    fn get(&self, agent_id: &AgentID) -> Result<Option<Hash>, HashRepositoryError> {
        let mut conf_path = self.conf_path.clone();
        let hash_path = self.hash_file_path(agent_id, &mut conf_path);
        debug!("Reading hash file at {}", hash_path.to_string_lossy());
        // Reading and failing to get a hash should not interrupt the program execution,
        // but should indicate that there is no hash info available.
        // Hence, we discard the error variant and transform it to a `None: Option<String>`.
        let contents = self.file_rw.read(hash_path).ok();
        // We attempt to parse the `Hash` from the String if we got it, failing if we cannot parse.
        let result = contents.map(|s| serde_yaml::from_str(&s)).transpose();
        Ok(result?)
    }
}

impl HashRepositoryFile {
    pub fn new_sub_agent_repository() -> Self {
        HashRepositoryFile::new(REMOTE_AGENT_DATA_DIR().to_string())
    }
}

impl<F, D> HashRepositoryFile<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn hash_file_path<'a>(&'a self, agent_id: &AgentID, path: &'a mut PathBuf) -> &Path {
        let hash_file = if agent_id.is_super_agent_id() {
            HASH_FILE_NAME.to_string()
        } else {
            format!("{}/{}", agent_id.get(), HASH_FILE_NAME)
        };
        path.push(hash_file);
        path
    }

    // Wrapper for linux with unix specific permissions
    #[cfg(target_family = "unix")]
    fn write(&self, path: &Path, content: String) -> Result<(), WriteError> {
        use crate::sub_agent::values::FILE_PERMISSIONS;

        self.file_rw
            .write(path, content, Permissions::from_mode(FILE_PERMISSIONS))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod test {
    use super::{Hash, HashRepository, HashRepositoryFile, DIRECTORY_PERMISSIONS, HASH_FILE_NAME};
    use crate::{sub_agent::values::FILE_PERMISSIONS, super_agent::config::AgentID};
    use fs::directory_manager::mock::MockDirectoryManagerMock;
    use fs::directory_manager::DirectoryManager;
    use fs::file_reader::FileReader;
    use fs::mock::MockLocalFile;
    use fs::writer_file::FileWriter;
    use std::fs::Permissions;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    impl<F, D> HashRepositoryFile<F, D>
    where
        D: DirectoryManager,
        F: FileWriter + FileReader,
    {
        pub fn with_mocks(file_rw: F, directory_manager: D, conf_path: PathBuf) -> Self {
            HashRepositoryFile {
                file_rw,
                conf_path,
                directory_manager,
            }
        }
    }

    #[test]
    fn test_save_and_get_hash() {
        let some_path = PathBuf::from("some/path");
        let mut file_rw = MockLocalFile::new();
        let file_permissions = Permissions::from_mode(FILE_PERMISSIONS);
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut hash = Hash::new("123456789".to_string());
        hash.apply();

        // This indentation and the single quotes is to match serde yaml
        let content = r#"hash: '123456789'
state: applied
"#;

        let mut expected_path = some_path.clone();
        expected_path.push(format!("{}/{}", agent_id.get(), HASH_FILE_NAME));

        file_rw.should_read(expected_path.as_path(), content.to_string());
        file_rw.should_write(
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

        let hash_repository = HashRepositoryFile::with_mocks(file_rw, dir_manager, some_path);

        let result = hash_repository.save(&agent_id, &hash);
        assert!(result.is_ok());

        let result = hash_repository.get(&agent_id);
        assert_eq!(Some(hash), result.unwrap());
    }
}
