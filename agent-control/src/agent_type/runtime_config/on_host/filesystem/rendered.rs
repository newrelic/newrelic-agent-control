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
    DirContentFromMap(HashMap<PathBuf, String>),
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

/// Creates `dir` (and any missing parents), with error context. Safe if it already exists.
fn ensure_dir(
    dir_manager: &impl DirectoryManager,
    dir: &Path,
) -> Result<(), FileSystemEntriesError> {
    trace!("Creating directory {}", dir.display());
    dir_manager
        .create(dir)
        .map_err(|err| FileSystemEntriesError(format!("creating directory {dir:?}: {err}")))
}

/// Writes `content` to `path`, creating its parent directory first. Overwrites an existing file.
fn write_file(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    path: &Path,
    content: &str,
) -> Result<(), FileSystemEntriesError> {
    trace!("Writing filesystem entry to {}", path.display());
    // We ensure the parent exists even if the dir is declared independently.
    let parent = path
        .parent()
        .ok_or_else(|| FileSystemEntriesError(format!("{} has no parent dir", path.display())))?;
    ensure_dir(dir_manager, parent)?;
    file_writer
        .write(path, content.to_owned())
        .map_err(|err| FileSystemEntriesError(format!("creating file {path:?}: {err}")))
}

fn write_entry(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    path: &Path,
    entry: &RenderedEntry,
) -> Result<(), FileSystemEntriesError> {
    match entry {
        RenderedEntry::File(content) => write_file(file_writer, dir_manager, path, content),
        RenderedEntry::Dir(children) => {
            ensure_dir(dir_manager, path)?;
            for (sub_path, child) in children {
                let child_path = path.join(sub_path);
                trace!("Recursing into child entry {}", child_path.display());
                write_entry(file_writer, dir_manager, &child_path, child)?;
            }
            Ok(())
        }
        RenderedEntry::DirContentFromMap(files) => {
            ensure_dir(dir_manager, path)?;
            for (file_name, content) in files {
                write_file(file_writer, dir_manager, &path.join(file_name), content)?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);
