use crate::agent_type::runtime_config::on_host::filesystem::{DirEntriesMap, SafePath};
use ::fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::trace;

pub const MANIFEST_FILE_NAME: &str = ".supervisor-manifest.json";

/// Tracks which relative directory paths inside the agent filesystem dir should survive
/// agent removal. Written as JSON alongside the agent's filesystem entries.
#[derive(Debug, Serialize, Deserialize)]
pub struct FilesystemManifest {
    pub persistent_dirs: Vec<PathBuf>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem {
    pub(super) ephemeral: HashMap<SafePath, DirEntriesMap>,
    pub(super) persistent: HashMap<SafePath, DirEntriesMap>,
    /// Absolute path to the agent's dedicated filesystem directory
    /// (e.g. `{remote_dir}/filesystem/{agent_id}`). Used to derive relative persistent paths
    /// written to the manifest and to locate the manifest file itself.
    pub(super) filesystem_dir: PathBuf,
}

impl FileSystem {
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        write_ephemeral_dirs(&self.ephemeral, file_writer, dir_manager)?;
        write_persistent_dirs(&self.persistent, file_writer, dir_manager)?;

        if !self.filesystem_dir.as_os_str().is_empty() {
            self.write_manifest(file_writer, dir_manager)?;
        }
        Ok(())
    }

    /// Writes a JSON manifest at `{filesystem_dir}/.supervisor-manifest.json` listing the
    /// relative paths of every persistent directory so the on-host resource cleaner can
    /// determine what to keep after agent removal.
    fn write_manifest(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        let persistent_dirs: Vec<PathBuf> = self
            .persistent
            .keys()
            .filter_map(|abs_path| {
                abs_path
                    .as_ref()
                    .strip_prefix(&self.filesystem_dir)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .collect();

        let manifest = FilesystemManifest { persistent_dirs };
        let manifest_json = serde_json::to_string(&manifest)
            .map_err(|e| FileSystemEntriesError(format!("serializing filesystem manifest: {e}")))?;

        dir_manager.create(&self.filesystem_dir).map_err(|err| {
            FileSystemEntriesError(format!(
                "creating filesystem dir {:?}: {err}",
                self.filesystem_dir
            ))
        })?;

        let manifest_path = self.filesystem_dir.join(MANIFEST_FILE_NAME);
        file_writer
            .write(&manifest_path, manifest_json)
            .map_err(|err| {
                FileSystemEntriesError(format!("writing manifest {:?}: {err}", manifest_path))
            })
    }
}

/// Clears each directory before writing its contents.
/// This ensures files no longer present in the config are removed from disk.
fn write_ephemeral_dirs(
    dirs: &HashMap<SafePath, DirEntriesMap>,
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    dirs.iter().try_for_each(|(dir_path, dir_entries)| {
        // Delete existing contents so stale files (e.g. removed integrations) are cleaned up.
        // `delete` is a no-op when the directory does not yet exist.
        dir_manager.delete(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("clearing ephemeral directory {dir_path:?}: {err}"))
        })?;
        dir_manager.create(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("creating directory {dir_path:?}: {err}"))
        })?;
        dir_entries
            .0
            .iter()
            .try_for_each(|(sub_path, file_content)| {
                let file_path = dir_path.as_ref().join(sub_path);
                create_file(file_writer, dir_manager, &file_path, file_content)
            })
    })
}

/// Writes directory contents additively — existing files are overwritten but never deleted.
/// Used for persistent directories whose contents must survive config updates.
fn write_persistent_dirs(
    dirs: &HashMap<SafePath, DirEntriesMap>,
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    dirs.iter().try_for_each(|(dir_path, dir_entries)| {
        dir_manager.create(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("creating directory {dir_path:?}: {err}"))
        })?;
        dir_entries
            .0
            .iter()
            .try_for_each(|(sub_path, file_content)| {
                let file_path = dir_path.as_ref().join(sub_path);
                create_file(file_writer, dir_manager, &file_path, file_content)
            })
    })
}

fn create_file(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    file_path: &PathBuf,
    file_content: &String,
) -> Result<(), FileSystemEntriesError> {
    trace!("Writing filesystem entry to {}", file_path.display());
    let parent_dir = file_path.parent().ok_or_else(|| {
        FileSystemEntriesError(format!("{} has no parent dir", file_path.display()))
    })?;
    dir_manager.create(parent_dir).map_err(|err| {
        FileSystemEntriesError(format!("creating directory {parent_dir:?}: {err}"))
    })?;
    // Will overwrite files if they already exist!
    file_writer
        .write(file_path.as_path(), file_content.to_owned())
        .map_err(|err| FileSystemEntriesError(format!("creating file {file_path:?}: {err}")))
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);
