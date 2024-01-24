use fs::LocalFile;
use newrelic_super_agent::super_agent::config::AgentID;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::debug;

use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::writer_file::{FileWriter, WriteError};
use newrelic_super_agent::super_agent::defaults::{LOCAL_AGENT_DATA_DIR, VALUES_DIR, VALUES_FILE};

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: `{0}`")]
    FileError(#[from] WriteError),
}

#[cfg(target_family = "unix")]
pub(crate) const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

pub struct ValuesPersisterFile<C = DirectoryManagerFs, F = LocalFile>
where
    C: DirectoryManager,
    F: FileWriter,
{
    file_writer: F,
    directory_manager: C,
    local_agent_data_dir: PathBuf,
}

impl ValuesPersisterFile<DirectoryManagerFs> {
    pub fn new(data_dir: &Path) -> Self {
        ValuesPersisterFile {
            file_writer: LocalFile,
            directory_manager: DirectoryManagerFs::default(),
            local_agent_data_dir: PathBuf::from(data_dir),
        }
    }
}

impl Default for ValuesPersisterFile<DirectoryManagerFs> {
    fn default() -> Self {
        ValuesPersisterFile::new(Path::new(LOCAL_AGENT_DATA_DIR))
    }
}

#[cfg_attr(test, mockall::automock)]
impl<C> ValuesPersisterFile<C>
where
    C: DirectoryManager,
{
    pub fn persist_values_file(
        &self,
        agent_id: &AgentID,
        values_content: &str,
    ) -> Result<(), PersistError> {
        let mut path = PathBuf::from(&self.local_agent_data_dir);
        path.push(agent_id);
        if !path.exists() {
            self.create_directory(&path)?;
        }
        path.push(VALUES_DIR);
        if !path.exists() {
            self.create_directory(&path)?;
        }
        path.push(VALUES_FILE);

        debug!("writing to file {:?}", path.as_path());

        Ok(self.write(path.as_path(), values_content)?)
    }

    // Wrapper for linux with unix specific permissions
    #[cfg(target_family = "unix")]
    fn write(&self, path: &Path, content: &str) -> Result<(), WriteError> {
        self.file_writer.write(
            path,
            content.to_string(),
            Permissions::from_mode(FILE_PERMISSIONS),
        )
    }

    // Wrapper for linux with unix specific permissions
    #[cfg(target_family = "unix")]
    fn create_directory(&self, path: &Path) -> Result<(), DirectoryManagementError> {
        self.directory_manager
            .create(path, Permissions::from_mode(DIRECTORY_PERMISSIONS))
    }

    #[cfg(target_family = "windows")]
    fn write(&self, path: &Path, content: &str) -> Result<(), WriteError> {
        todo!()
    }

    #[cfg(target_family = "windows")]
    fn create_directory(&self, path: &Path) -> Result<(), WriteError> {
        todo!()
    }
}
