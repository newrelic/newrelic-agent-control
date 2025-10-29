use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG};
use crate::on_host::file_store::build_config_name;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::writer_file::{FileWriter, WriteError};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("directory error: {0}")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: {0}")]
    FileError(#[from] WriteError),
}

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
    pub fn new(data_dir: PathBuf) -> Self {
        ValuesPersisterFile {
            file_writer: LocalFile,
            directory_manager: DirectoryManagerFs,
            local_agent_data_dir: data_dir,
        }
    }
}

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
        path.push(FOLDER_NAME_LOCAL_DATA);
        path.push(agent_id);
        if !path.exists() {
            self.create_directory(&path)?;
        }
        path.push(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));

        debug!("writing to file {:?}", path.as_path());

        Ok(self.write(path.as_path(), values_content)?)
    }

    // Wrapper for linux with unix specific permissions
    fn write(&self, path: &Path, content: &str) -> Result<(), WriteError> {
        self.file_writer.write(path, content.to_string())
    }

    // Wrapper for linux with unix specific permissions
    fn create_directory(&self, path: &Path) -> Result<(), DirectoryManagementError> {
        self.directory_manager.create(path)
    }
}
