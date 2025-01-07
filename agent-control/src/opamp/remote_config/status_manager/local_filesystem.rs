use fs::{
    directory_manager::{DirectoryManager, DirectoryManagerFs},
    file_reader::{FileReader, FileReaderError},
    writer_file::FileWriter,
    LocalFile,
};
use opamp_client::operation::capabilities::Capabilities;
use serde::de::DeserializeOwned;
use std::{
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::RwLock,
};
use tracing::{debug, error, trace, warn};

use crate::{
    agent_control::{
        config::AgentID,
        defaults::{
            AGENT_CONTROL_CONFIG_FILENAME, REMOTE_CONFIG_STATUS_FILENAME, SUB_AGENT_DIR,
            VALUES_DIR, VALUES_FILENAME,
        },
    },
    opamp::remote_config::status::AgentRemoteConfigStatus,
    values::yaml_config::{has_remote_management, YAMLConfig},
};

use super::{error::ConfigStatusManagerError, ConfigStatusManager};

#[cfg(target_family = "unix")]
const FILE_PERMISSIONS: u32 = 0o600;

#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

pub struct FileSystemConfigStatusManager<S = DirectoryManagerFs, F = LocalFile>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    directory_manager: S,
    file_rw: F,
    remote_config_path: Option<PathBuf>,
    local_config_path: PathBuf,
    rw_lock: RwLock<()>, // FIXME enclose the types that are the actual resource to be locked?
}

impl FileSystemConfigStatusManager {
    pub fn new(local_config_path: PathBuf) -> Self {
        Self {
            local_config_path,
            directory_manager: DirectoryManagerFs,
            file_rw: LocalFile,
            remote_config_path: None,
            rw_lock: RwLock::new(()),
        }
    }

    pub fn with_remote(self, remote_path: PathBuf) -> Self {
        // TODO: Should also set AcceptsRemoteConfig capability? Requires change in opamp-rs.
        Self {
            remote_config_path: Some(remote_path),
            ..self
        }
    }
}

impl<S, F> FileSystemConfigStatusManager<S, F>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn get_local_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        if agent_id.is_agent_control_id() {
            return self.local_config_path.join(AGENT_CONTROL_CONFIG_FILENAME);
        }
        concatenate_sub_agent_values_dir_path(&self.local_config_path, agent_id)
    }

    fn get_remote_status_file_path(&self, agent_id: &AgentID) -> Option<PathBuf> {
        self.remote_config_path.as_ref().map(|p| {
            if agent_id.is_agent_control_id() {
                p.join(AGENT_CONTROL_CONFIG_FILENAME)
            } else {
                concatenate_sub_agent_status_dir_path(p, agent_id)
            }
        })
    }

    fn retrieve_values_if_present<D>(
        &self,
        path: &Path,
    ) -> Result<Option<D>, ConfigStatusManagerError>
    where
        D: DeserializeOwned,
    {
        match self.file_rw.read(path) {
            Err(FileReaderError::FileNotFound(e)) => {
                trace!("file not found: {}", e);
                Ok(None)
            }
            Ok(res) => {
                Ok(Some(serde_yaml::from_str(&res).map_err(|e| {
                    ConfigStatusManagerError::Retrieval(e.to_string())
                })?))
            }
            Err(e) => {
                // log unexpected errors for now. When to propagate?
                error!("error retrieving file at {}", path.display());
                Err(ConfigStatusManagerError::Retrieval(e.to_string()))
            }
        }
    }

    fn ensure_directory_exists(
        &self,
        values_file_path: &Path,
    ) -> Result<(), ConfigStatusManagerError> {
        let parent_dir = values_file_path.parent().ok_or_else(|| {
            ConfigStatusManagerError::Store("values file path has no parent directory".to_string())
        })?;

        if !parent_dir.exists() {
            self.directory_manager
                .create(parent_dir, Permissions::from_mode(DIRECTORY_PERMISSIONS))
                .map_err(|e| ConfigStatusManagerError::Store(e.to_string()))?;
        }
        Ok(())
    }
}

pub fn concatenate_sub_agent_values_dir_path(dir: &Path, agent_id: &AgentID) -> PathBuf {
    dir.join(SUB_AGENT_DIR)
        .join(agent_id)
        .join(VALUES_DIR)
        .join(VALUES_FILENAME)
}

pub fn concatenate_sub_agent_status_dir_path(dir: &Path, agent_id: &AgentID) -> PathBuf {
    dir.join(SUB_AGENT_DIR)
        .join(agent_id)
        .join(REMOTE_CONFIG_STATUS_FILENAME)
}

impl<S, F> ConfigStatusManager for FileSystemConfigStatusManager<S, F>
where
    S: DirectoryManager + Send + Sync + 'static,
    F: FileWriter + FileReader + Send + Sync + 'static,
{
    fn retrieve_local_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, ConfigStatusManagerError> {
        let _read_guard = self.rw_lock.read().expect(
            "FileSystemConfigStatusManager read in retrieve_local_config call: lock poisoned",
        );

        let local_values_path = self.get_local_values_file_path(agent_id);
        self.retrieve_values_if_present(&local_values_path)
    }

    fn retrieve_remote_status(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<AgentRemoteConfigStatus>, ConfigStatusManagerError> {
        if !has_remote_management(capabilities) {
            return Ok(None);
        }
        let Some(remote_dir_path) = self.get_remote_status_file_path(agent_id) else {
            return Ok(None);
        };

        let _read_guard = self.rw_lock.read().expect(
            "FileSystemConfigStatusManager read in retrieve_remote_status call: lock poisoned",
        );
        self.retrieve_values_if_present(&remote_dir_path)
    }

    fn store_remote_status(
        &self,
        agent_id: &AgentID,
        status: &AgentRemoteConfigStatus,
    ) -> Result<(), ConfigStatusManagerError> {
        let _write_guard = self.rw_lock.write().expect(
            "FileSystemConfigStatusManager write in store_remote_status call: lock poisoned",
        );

        let Some(values_file_path) = self.get_remote_status_file_path(agent_id) else {
            unreachable!("remote values file path not found"); // FIXME review design
        };

        self.ensure_directory_exists(&values_file_path)?;

        let content = serde_yaml::to_string(status)
            .map_err(|e| ConfigStatusManagerError::Store(e.to_string()))?;

        self.file_rw
            .write(
                &values_file_path,
                content,
                Permissions::from_mode(FILE_PERMISSIONS),
            )
            .map_err(|e| ConfigStatusManagerError::Store(e.to_string()))
    }

    fn delete_remote_status(&self, agent_id: &AgentID) -> Result<(), ConfigStatusManagerError> {
        let _write_guard = self.rw_lock.write().expect(
            "FileSystemConfigStatusManager write in delete_remote_status call: lock poisoned",
        );

        let Some(remote_status_file_path) = self.get_remote_status_file_path(agent_id) else {
            unreachable!("remote values file path not found"); // FIXME review design
        };

        if !remote_status_file_path.exists() {
            // This should not happen, but I guess we don't want to fail if the file is already gone
            // for some reason.
            warn!(
                "attempted to remove remote status file at {}, but it does not exist",
                remote_status_file_path.display()
            );
            return Ok(());
        }

        debug!(
            "deleting remote status file at {}",
            remote_status_file_path.display()
        );
        std::fs::remove_file(remote_status_file_path)
            .map_err(|e| ConfigStatusManagerError::Deletion(e.to_string()))
    }
}
