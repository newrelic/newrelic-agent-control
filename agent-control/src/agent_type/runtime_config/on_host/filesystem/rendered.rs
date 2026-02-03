use crate::agent_type::runtime_config::on_host::filesystem::{DirEntriesMap, SafePath};
use ::fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::{collections::HashMap, io};
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
        self.0.iter().try_for_each(|(path, dir_entries)| {
            // Create the base directory so that we support empty directories
            dir_manager.create(path.as_ref())?;
            create_files(file_writer, dir_manager, path, dir_entries)?;
            Ok(())
        })
    }
}

fn create_files(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    path: &SafePath,
    dir_entries: &DirEntriesMap,
) -> Result<(), FileSystemEntriesError> {
    dir_entries
        .0
        .iter()
        .try_for_each(|(sub_path, file_content)| {
            let file_path = path.as_ref().join(sub_path);

            trace!("Writing filesystem entry to {}", file_path.display());
            let parent_dir = file_path.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidFilename,
                    format!("{} has no parent dir", file_path.display()),
                )
            })?;
            dir_manager.create(parent_dir)?;
            // Will overwrite files if they already exist!
            file_writer.write(file_path.as_path(), file_content.to_owned())
        })?;
    Ok(())
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(io::Error);

impl From<io::Error> for FileSystemEntriesError {
    fn from(value: io::Error) -> Self {
        FileSystemEntriesError(value)
    }
}
