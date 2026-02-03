use crate::agent_type::runtime_config::on_host::filesystem::{DirEntriesMap, SafePath};
use ::fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use tracing::trace;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(pub(super) HashMap<SafePath, DirEntriesMap>);

impl FileSystem {
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        self.0.iter().try_for_each(|(dir_path, dir_entries)| {
            // Create the base directory so that we support empty directories
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
