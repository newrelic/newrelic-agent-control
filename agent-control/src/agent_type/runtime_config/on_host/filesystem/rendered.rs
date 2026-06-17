use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::trace;

/// Rendered filesystem tree, ready to be materialized on disk.
///
/// Top-level keys are absolute paths but children inside [`RenderedEntry::Dir`] are kept relative
/// to their parent — recursion in [`FileSystem::write`] joins them onto the parent path.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(pub(super) HashMap<PathBuf, RenderedEntry>);

#[derive(Debug, Clone, PartialEq)]
pub enum RenderedEntry {
    File(String),
    Dir(HashMap<PathBuf, RenderedEntry>),
}

impl FileSystem {
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.0 {
            write_entry(file_writer, dir_manager, path, entry)?;
        }
        Ok(())
    }
}

fn write_entry(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    path: &Path,
    entry: &RenderedEntry,
) -> Result<(), FileSystemEntriesError> {
    match entry {
        RenderedEntry::File(content) => {
            trace!("Writing filesystem entry to {}", path.display());
            let parent = path.parent().ok_or_else(|| {
                FileSystemEntriesError(format!("{} has no parent dir", path.display()))
            })?;
            // safe even if the dir already exists.
            dir_manager.create(parent).map_err(|err| {
                FileSystemEntriesError(format!("creating directory {parent:?}: {err}"))
            })?;
            // Will overwrite the file if it already exists.
            file_writer
                .write(path, content.to_owned())
                .map_err(|err| FileSystemEntriesError(format!("creating file {path:?}: {err}")))
        }
        RenderedEntry::Dir(children) => {
            dir_manager.create(path).map_err(|err| {
                FileSystemEntriesError(format!("creating directory {path:?}: {err}"))
            })?;
            for (sub_path, child) in children {
                write_entry(file_writer, dir_manager, &path.join(sub_path), child)?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);
