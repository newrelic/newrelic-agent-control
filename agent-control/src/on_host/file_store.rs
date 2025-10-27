use std::{
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
    sync::RwLock,
};

use fs::{
    LocalFile,
    directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs},
    file_reader::{FileReader, FileReaderError},
    writer_file::FileWriter,
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{error, trace};

use crate::{
    agent_control::{
        agent_id::AgentID,
        defaults::{
            FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
            STORE_KEY_OPAMP_DATA_CONFIG,
        },
        run::BasePaths,
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
    remote_dir: PathBuf,
    local_dir: PathBuf,
    rw_lock: RwLock<()>,
}

impl From<BasePaths> for FileStore<LocalFile, DirectoryManagerFs> {
    fn from(
        BasePaths {
            local_dir,
            remote_dir,
            ..
        }: BasePaths,
    ) -> Self {
        let file_rw = LocalFile;
        let directory_manager = DirectoryManagerFs;
        let rw_lock = RwLock::new(());

        Self {
            file_rw,
            directory_manager,
            local_dir,
            remote_dir,
            rw_lock,
        }
    }
}

// Proposed API
impl<F, D> FileStore<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(
        file_rw: F,
        directory_manager: D,
        BasePaths {
            local_dir,
            remote_dir,
            ..
        }: BasePaths,
    ) -> Self {
        let rw_lock = RwLock::new(());
        Self {
            file_rw,
            directory_manager,
            local_dir,
            remote_dir,
            rw_lock,
        }
    }

    pub fn get_local_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.local_dir
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG))
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.remote_dir
            .join(FOLDER_NAME_FLEET_DATA)
            .join(agent_id)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG))
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
        let _read_guard = self.rw_lock.read().unwrap();

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
        self.get(self.get_remote_values_file_path(agent_id))
    }

    pub fn get_local_data<T>(&self, agent_id: &AgentID) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        self.get(self.get_local_values_file_path(agent_id))
    }

    /// Stores data in the specified StoreKey of an Agent store.
    pub fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &Path, data: &T) -> Result<(), Error>
    where
        T: Serialize,
    {
        // #[allow(clippy::readonly_write_lock)]
        // let _write_guard = self.rw_lock.write().unwrap();

        // let data_as_string = serde_yaml::to_string(data)?;
        // let configmap_name = K8sStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        // self.k8s_client.set_configmap_key(
        //     &configmap_name,
        //     self.namespace.as_str(),
        //     Labels::new(agent_id).get(),
        //     key,
        //     &data_as_string,
        // )
        unimplemented!();
    }

    /// Delete data in the specified StoreKey of an Agent store.
    pub fn delete_opamp_data(&self, agent_id: &AgentID, key: &Path) -> Result<(), Error> {
        // #[allow(clippy::readonly_write_lock)]
        // let _write_guard = self.rw_lock.write().unwrap();

        // let configmap_name = K8sStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        // self.k8s_client
        //     .delete_configmap_key(&configmap_name, self.namespace.as_str(), key)
        unimplemented!();
    }
}
