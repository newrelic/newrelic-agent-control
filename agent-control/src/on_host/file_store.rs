use std::{
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
    sync::RwLock,
};

use fs::{
    directory_manager::{DirectoryManagementError, DirectoryManager},
    file_reader::{FileReader, FileReaderError},
    writer_file::FileWriter,
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{debug, error, trace};

use crate::{
    agent_control::{
        agent_id::AgentID,
        defaults::{
            FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
            STORE_KEY_OPAMP_DATA_CONFIG,
        },
    },
    opamp::instance_id::on_host::storer::build_config_name,
};

pub struct FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    directory_manager: D,
    file_rw: F,
    remote_dir: RwLock<RemoteDir>, // Will write to this path
    local_dir: LocalDir,           // Read-only, no need to sync?
}

pub struct LocalDir(PathBuf);

impl LocalDir {
    pub fn get_local_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.0
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG))
    }
}

pub struct RemoteDir(PathBuf);

impl RemoteDir {
    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.0
            .join(FOLDER_NAME_FLEET_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG))
    }
}

// Proposed API
impl<F, D> FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(file_rw: F, directory_manager: D, local_dir: PathBuf, remote_dir: PathBuf) -> Self {
        let remote_dir = RwLock::new(RemoteDir(remote_dir));
        let local_dir = LocalDir(local_dir);
        Self {
            file_rw,
            directory_manager,
            local_dir,
            remote_dir,
        }
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(&self, path: PathBuf) -> Result<Option<String>, FileReaderError> {
        let values_result = self.file_rw.read(path.as_path());
        match values_result {
            Ok(res) => Ok(Some(res)),
            Err(FileReaderError::FileNotFound(e)) => {
                trace!("file not found! {e}");
                // actively fallback to load local file
                Ok(None)
            }
            Err(err) => {
                // we log any unexpected error for now but maybe we should propagate it
                error!("error loading file {}", path.display());
                Err(err)
            }
        }
    }

    /// ensures directory exists
    fn ensure_directory_existence(
        &self,
        values_file_path: &Path,
    ) -> Result<(), DirectoryManagementError> {
        // This implementation is missing two cases in which the parent "does not exist":
        // 1. `values_file_path` is the root directory or a prefix (e.g. "/", "C:\")
        // 2. `values_file_path` is the empty string
        // In both cases this is a no-op, but should it?
        if let Some(parent) = values_file_path.parent()
            && !parent.exists()
        {
            self.directory_manager.create(parent)?;
        }
        Ok(())
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    fn get<T>(&self, key: PathBuf) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        self.load_file_if_present(key)
            .map_err(Error::other) // TODO: Address this!
            .and_then(|maybe_values| {
                maybe_values
                    .map(|s| serde_yaml::from_str(&s))
                    .transpose()
                    .map_err(|err| Error::new(ErrorKind::InvalidData, err)) // TODO: Address this!
            })
    }

    pub fn get_opamp_data<T>(&self, agent_id: &AgentID) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        let remote_dir = self.remote_dir.read().unwrap();
        self.get(remote_dir.get_remote_values_file_path(agent_id))
    }

    pub fn get_local_data<T>(&self, agent_id: &AgentID) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        self.get(self.local_dir.get_local_values_file_path(agent_id))
    }

    /// Stores data in the specified StoreKey of an Agent store.
    pub fn set_opamp_data<T>(&self, agent_id: &AgentID, data: &T) -> Result<(), Error>
    where
        T: Serialize,
    {
        // I'm writing the locked file, not mutating the path
        // I think the OS will handle concurrent write/delete fine from all
        // threads/subprocesses of the program, but just in case. We can revisit later.
        #[allow(clippy::readonly_write_lock)]
        let remote_dir = self.remote_dir.write().unwrap();

        let remote_values_path = remote_dir.get_remote_values_file_path(agent_id);

        self.ensure_directory_existence(&remote_values_path)
            .map_err(|err| {
                Error::other(format!(
                    "error ensuring directory existence for {}: {}",
                    remote_values_path.display(),
                    err
                ))
            })?;
        let content =
            serde_yaml::to_string(data).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

        self.file_rw
            .write(remote_values_path.as_path(), content)
            .map_err(|err| {
                Error::other(format!(
                    "error writing file {}: {}",
                    remote_values_path.display(),
                    err
                ))
            })
    }

    /// Delete data of an Agent store.
    pub fn delete_opamp_data(&self, agent_id: &AgentID) -> Result<(), Error> {
        // I'm writing (deleting) the locked file, not mutating the path
        // I think the OS will handle concurrent write/delete fine from all
        // threads/subprocesses of the program, but just in case. We can revisit later.
        #[allow(clippy::readonly_write_lock)]
        let remote_dir = self.remote_dir.write().unwrap();

        let remote_path_file = remote_dir.get_remote_values_file_path(agent_id);
        if remote_path_file.exists() {
            debug!("deleting remote config: {:?}", remote_path_file);
            std::fs::remove_file(remote_path_file)?;
        }
        Ok(())
    }
}
#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;

    use fs::{
        directory_manager::DirectoryManager, file_reader::FileReader, writer_file::FileWriter,
    };

    use crate::agent_control::agent_id::AgentID;

    use super::*;

    impl<F, S> FileStore<F, S>
    where
        S: DirectoryManager,
        F: FileWriter + FileReader,
    {
        pub fn get_testing_path(&self, agent_id: &AgentID, remote_enabled: bool) -> PathBuf {
            if remote_enabled {
                self.remote_dir
                    .read()
                    .unwrap()
                    .get_remote_values_file_path(agent_id)
            } else {
                self.local_dir.get_local_values_file_path(agent_id)
            }
        }
    }

    impl From<PathBuf> for LocalDir {
        fn from(path: PathBuf) -> Self {
            Self(path)
        }
    }

    impl From<RemoteDir> for PathBuf {
        fn from(remote_dir: RemoteDir) -> Self {
            remote_dir.0
        }
    }

    impl From<PathBuf> for RemoteDir {
        fn from(path: PathBuf) -> Self {
            Self(path)
        }
    }

    impl From<LocalDir> for PathBuf {
        fn from(local_dir: LocalDir) -> Self {
            local_dir.0
        }
    }
}
