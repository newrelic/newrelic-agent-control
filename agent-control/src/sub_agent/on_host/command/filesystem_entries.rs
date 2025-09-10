use std::{collections::HashMap, fs::Permissions, io, os::unix::fs::PermissionsExt, path::PathBuf};

use ::fs::{
    directory_manager::{DirectoryManagementError, DirectoryManager},
    writer_file::{FileWriter, WriteError},
};
use thiserror::Error;

use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub enum FileSystemEntriesError {
    Io(io::Error),
    DirManagement(DirectoryManagementError),
    FileWrite(WriteError),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderedFileSystemEntries(HashMap<PathBuf, String>);

impl From<FileSystem> for RenderedFileSystemEntries {
    fn from(value: FileSystem) -> Self {
        Self(value.rendered())
    }
}

impl RenderedFileSystemEntries {
    #[cfg(target_family = "unix")]
    const FILE_PERMISSIONS: u32 = 0o600;
    #[cfg(target_family = "unix")]
    const DIRECTORY_PERMISSIONS: u32 = 0o700;
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        self.0.iter().try_for_each(|(path, content)| {
            let parent_dir = path
                .parent()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidFilename,
                        format!("{} has no parent dir", path.display()),
                    )
                })
                .map_err(FileSystemEntriesError::Io)?;
            dir_manager
                .create(
                    parent_dir,
                    Permissions::from_mode(Self::DIRECTORY_PERMISSIONS),
                )
                .map_err(FileSystemEntriesError::DirManagement)?;
            // Will overwrite files if they already exist!
            file_writer
                .write(
                    path,
                    content.to_owned(),
                    Permissions::from_mode(Self::FILE_PERMISSIONS),
                )
                .map_err(FileSystemEntriesError::FileWrite)
        })
    }
}
