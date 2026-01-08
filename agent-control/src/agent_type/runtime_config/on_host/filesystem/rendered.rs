use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use ::fs::{
    directory_manager::{DirectoryManagementError, DirectoryManager},
    file::writer::{FileWriter, WriteError},
};
use thiserror::Error;
use tracing::trace;

use crate::agent_type::runtime_config::on_host::filesystem::{DirEntriesMap, SafePath};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(pub(super) HashMap<SafePath, DirEntriesType>);

impl FileSystem {
    /// Returns the internal file entries as a [`HashMap<PathBuf, String>`] so they can
    /// be written into the actual host filesystem.
    pub(super) fn expand_paths(self) -> HashMap<PathBuf, String> {
        self.0
            .into_iter()
            .flat_map(|(dir_path, dir_entries)| dir_entries.expand_paths_with(&dir_path))
            .collect()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DirEntriesType {
    /// A directory with a fixed set of entries (i.e. files). Each entry's content can be templated.
    /// E.g.
    /// ```yaml
    /// "my/dir":
    ///   filepath1: "file1 content with ${nr-var:some_var}"
    ///   filepath2: "file2 content"
    /// ```
    FixedWithTemplatedContent(HashMap<SafePath, String>),

    /// A directory with a fully templated set of entries, where it's expected that a full template
    /// is provided that renders to a valid YAML mapping of a safe [`PathBuf`] to [`String`].
    /// E.g.
    /// ```yaml
    /// "my/templated/dir":
    ///   ${nr-var:some_var_that_renders_to_a_yaml_mapping}
    /// ```
    FullyTemplated(DirEntriesMap),
}

impl DirEntriesType {
    /// Returns the directory entries as an iterator of [`HashMap<PathBuf, String>`] so they can
    /// be written into the actual host filesystem. Takes a base path to prepend to each entry.
    fn expand_paths_with(self, path: impl AsRef<Path>) -> HashMap<PathBuf, String> {
        let map = match self {
            Self::FixedWithTemplatedContent(map) => map,
            Self::FullyTemplated(tv) => tv.0,
        };

        map.into_iter()
            .map(|(k, v)| (path.as_ref().join(k), v))
            .collect()
    }
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub enum FileSystemEntriesError {
    Io(io::Error),
    DirManagement(DirectoryManagementError),
    FileWrite(WriteError),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileSystemEntries(HashMap<PathBuf, String>);

impl From<FileSystem> for FileSystemEntries {
    fn from(value: FileSystem) -> Self {
        Self(value.expand_paths())
    }
}

impl FileSystemEntries {
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        self.0.iter().try_for_each(|(path, content)| {
            trace!("Writing filesystem entry to {}", path.display());
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
                .create(parent_dir)
                .map_err(FileSystemEntriesError::DirManagement)?;
            // Will overwrite files if they already exist!
            file_writer
                .write(path, content.to_owned())
                .map_err(FileSystemEntriesError::FileWrite)
        })
    }
}
