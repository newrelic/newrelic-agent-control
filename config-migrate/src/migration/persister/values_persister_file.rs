use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use newrelic_super_agent::config::persister::config_persister::PersistError;
use newrelic_super_agent::config::super_agent_configs::AgentID;
use newrelic_super_agent::fs::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use newrelic_super_agent::fs::writer_file::WriteError;
#[cfg_attr(test, mockall_double::double)]
use newrelic_super_agent::fs::writer_file::WriterFile;
use newrelic_super_agent::super_agent::defaults::{LOCAL_AGENT_DATA_DIR, VALUES_FILENAME};

#[cfg(target_family = "unix")]
pub(crate) const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

pub struct ValuesPersisterFile<C = DirectoryManagerFs>
where
    C: DirectoryManager,
{
    file_writer: WriterFile,
    directory_manager: C,
    local_agent_data_dir: PathBuf,
}

impl ValuesPersisterFile<DirectoryManagerFs> {
    pub fn new(data_dir: &Path) -> Self {
        ValuesPersisterFile {
            file_writer: WriterFile::default(),
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
        self.create_directory(&path)?;

        path.push(VALUES_FILENAME);

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
