//! Rendered filesystem tree and the logic to materialize it on disk.
use fs::file::copier::FileCopier;
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{trace, warn};

/// Rendered filesystem tree, ready to be materialized on disk.
///
/// Top-level keys (`entries`) are absolute paths under the sub-agent's filesystem dir; children
/// inside a `Dir` are kept relative to their parent — recursion in [`FileSystem::write`] joins
/// them onto the parent path.
#[derive(Debug, Clone, PartialEq)]
pub struct FileSystem {
    pub(super) entries: HashMap<PathBuf, RenderedEntry>,
}

/// The source of a rendered file's bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum FileContent {
    /// Literal (rendered) content written verbatim.
    Text(String),
    /// An on-disk source file to copy byte-for-byte into place (used by `copy_from_file`).
    Copy(PathBuf),
}

/// A single rendered filesystem entry.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderedEntry {
    /// A file whose bytes come either from inline content or from a copied source file.
    File {
        /// Where the file's bytes come from.
        content: FileContent,
        /// The persistency attribute marking its lifecycle.
        persistent: bool,
    },
    /// A directory containing child entries keyed by their relative path.
    Dir {
        /// The dictionary containing each children path and the entry.
        children: HashMap<PathBuf, RenderedEntry>,
        /// The persistency attribute marking its lifecycle.
        persistent: bool,
    },
    /// A directory whose files were projected from a map (filename to content).
    DirContentFromMap {
        /// The dictionary containing all file paths and their content.
        files: HashMap<PathBuf, String>,
    },
}

impl RenderedEntry {
    fn persistent(&self) -> bool {
        match self {
            Self::File { persistent, .. } | Self::Dir { persistent, .. } => *persistent,
            // `dir_content_from_map` has no persistent flag: Agent Control re-renders its
            // projected files on every write, so it is always treated as ephemeral.
            Self::DirContentFromMap { .. } => false,
        }
    }

    /// Materializes this entry (and its subtree) on disk at `path`. `file_ops` provides both the
    /// write and copy capabilities (a single type, [`LocalFile`], implements both in production).
    fn write(
        &self,
        path: &Path,
        file_ops: &(impl FileWriter + FileCopier),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        match self {
            Self::File { content, .. } => match content {
                FileContent::Text(text) => write_file(file_ops, dir_manager, path, text),
                FileContent::Copy(source) => copy_file(file_ops, dir_manager, path, source),
            },
            Self::Dir { children, .. } => {
                ensure_dir(dir_manager, path)?;
                for (sub_path, child) in children {
                    let child_path = path.join(sub_path);
                    trace!("Recursing into child entry {}", child_path.display());
                    child.write(&child_path, file_ops, dir_manager)?;
                }
                Ok(())
            }
            Self::DirContentFromMap { files, .. } => {
                ensure_dir(dir_manager, path)?;
                for (file_name, content) in files {
                    write_file(file_ops, dir_manager, &path.join(file_name), content)?;
                }
                Ok(())
            }
        }
    }

    /// Deletes this entry's on-disk path if it is ephemeral. A persistent directory is kept, but
    /// the walk recurses so ephemeral descendants are still cleaned; an ephemeral ancestor is
    /// removed recursively, taking any persistent descendants with it.
    fn delete_ephemeral(&self, path: &Path) -> Result<(), FileSystemEntriesError> {
        if !self.persistent() {
            if path.exists() {
                delete_path(path)
                    .map_err(|err| {
                        FileSystemEntriesError(format!("deleting {}: {err}", path.display()))
                    })
                    .inspect_err(|err| warn!(?err, ?path, "delete_ephemeral failed"))?;
            }
            return Ok(());
        }
        // Persistent: keep this node, but its children may still be ephemeral.
        if let Self::Dir { children, .. } = self {
            for (sub, child) in children {
                child.delete_ephemeral(&path.join(sub))?;
            }
        }
        Ok(())
    }
}

impl FileSystem {
    pub(super) fn new(entries: HashMap<PathBuf, RenderedEntry>) -> Self {
        Self { entries }
    }

    /// Writes the declared tree under `base_dir`, overwriting any declared paths already on disk.
    pub fn write(
        &self,
        file_ops: &(impl FileWriter + FileCopier),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.write(path, file_ops, dir_manager)?;
        }
        Ok(())
    }

    /// Deletes the on-disk path of every ephemeral entry in the tree.
    /// A persistent entry whose ancestor is ephemeral is wiped along with the ancestor
    pub fn delete_ephemeral(&self) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.delete_ephemeral(path)?;
        }
        Ok(())
    }
}

/// Rendered shared filesystem tree, materialized under the base shared across sub-agents.
// TODO: there is no clean-up implemented at the moment (content written by agent remains there even if
// the corresponding agent is not present anymore).
#[derive(Debug, Clone, PartialEq)]
pub struct SharedFileSystem {
    entries: HashMap<PathBuf, RenderedEntry>,
}

impl SharedFileSystem {
    pub(super) fn new(entries: HashMap<PathBuf, RenderedEntry>) -> Self {
        Self { entries }
    }

    /// Materializes the declared tree on disk. Existing files are overwritten; nothing is pruned.
    pub fn write(
        &self,
        file_ops: &(impl FileWriter + FileCopier),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.write(path, file_ops, dir_manager)?;
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

/// Copies `source` to `path`, creating its parent directory first. Overwrites an existing file.
fn copy_file(
    file_copier: &impl FileCopier,
    dir_manager: &impl DirectoryManager,
    path: &Path,
    source: &Path,
) -> Result<(), FileSystemEntriesError> {
    trace!(
        "Copying filesystem entry from {} to {}",
        source.display(),
        path.display()
    );
    let parent = path
        .parent()
        .ok_or_else(|| FileSystemEntriesError(format!("{} has no parent dir", path.display())))?;
    ensure_dir(dir_manager, parent)?;
    file_copier
        .copy(source, path)
        .map_err(|err| FileSystemEntriesError(format!("copying {source:?} to {path:?}: {err}")))
}
/// Recursively deletes the file or directory at `path`.
fn delete_path(path: &Path) -> io::Result<()> {
    trace!("Deleting path {}", path.display());
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Error produced while writing the rendered filesystem tree to disk.
#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);

#[cfg(test)]
mod tests {
    use super::*;

    impl FileSystem {
        pub(crate) fn test_empty() -> Self {
            Self::new(HashMap::new())
        }
    }

    impl SharedFileSystem {
        pub(crate) fn test_empty() -> Self {
            Self::new(HashMap::new())
        }
    }
}
