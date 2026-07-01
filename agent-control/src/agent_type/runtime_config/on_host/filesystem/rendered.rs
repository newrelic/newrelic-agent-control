//! Rendered filesystem tree and the logic to materialize it on disk.
use crate::agent_type::runtime_config::on_host::managed_paths::{ManagedPaths, delete_path};
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{trace, warn};

/// Filename of the manifest Agent Control writes inside each sub-agent's filesystem dir.
/// The manifest records the absolute paths AC wrote on the previous successful write event so
/// the next write can compute "previously managed but no longer declared → delete".
///
/// Reserved name: agent-type definitions cannot declare an entry with this exact filename at any
/// level.
pub const MANAGED_PATHS_MANIFEST_FILENAME: &str = ".ac-managed-paths.json";

/// Rendered filesystem tree, ready to be materialized on disk.
///
/// Top-level keys (`entries`) are absolute paths under `base_dir`; children inside a `Dir` are
/// kept relative to their parent — recursion in [`FileSystem::write`] joins them onto the parent
/// path.
#[derive(Debug, Clone, PartialEq)]
pub struct FileSystem {
    pub(super) base_dir: PathBuf,
    pub(super) entries: HashMap<PathBuf, RenderedEntry>,
}

/// A single rendered filesystem entry.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderedEntry {
    /// A file with the given content.
    File {
        /// The rendered content from the file.
        content: String,
        /// The persistency attribute marking it's lifecicle.
        persistent: bool,
    },
    /// A directory containing child entries keyed by their relative path.
    Dir {
        /// The dictionary containing each children path and the entry.
        children: HashMap<PathBuf, RenderedEntry>,
        /// The persistency attribute marking it's lifecicle.
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

    /// Inserts this entry's path and all of its descendants' paths into `declared`.
    fn collect_declared(&self, path: &Path, declared: &mut HashSet<PathBuf>) {
        declared.insert(path.to_path_buf());
        match self {
            Self::File { .. } => {}
            Self::Dir { children, .. } => {
                for (sub, child) in children {
                    child.collect_declared(&path.join(sub), declared);
                }
            }
            Self::DirContentFromMap { files, .. } => {
                for sub in files.keys() {
                    declared.insert(path.join(sub));
                }
            }
        }
    }

    /// Materializes this entry (and its subtree) on disk at `path`.
    fn write(
        &self,
        path: &Path,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        match self {
            Self::File { content, .. } => write_file(file_writer, dir_manager, path, content),
            Self::Dir { children, .. } => {
                ensure_dir(dir_manager, path)?;
                for (sub_path, child) in children {
                    let child_path = path.join(sub_path);
                    trace!("Recursing into child entry {}", child_path.display());
                    child.write(&child_path, file_writer, dir_manager)?;
                }
                Ok(())
            }
            Self::DirContentFromMap { files, .. } => {
                ensure_dir(dir_manager, path)?;
                for (file_name, content) in files {
                    write_file(file_writer, dir_manager, &path.join(file_name), content)?;
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
    pub(super) fn new(base_dir: PathBuf, entries: HashMap<PathBuf, RenderedEntry>) -> Self {
        Self { base_dir, entries }
    }

    /// Reconciles the on-disk state under `base_dir` against the current declared tree, then
    /// writes the declared tree, then updates the manifest.
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        let managed = self.managed_paths();
        let prev_declared = managed.read();
        let curr_declared = self.collect_declared_paths();

        // Reconcile: delete paths AC owned previously but no longer declares.
        managed.prune_stale(&prev_declared, &curr_declared);

        for (path, entry) in &self.entries {
            entry.write(path, file_writer, dir_manager)?;
        }

        managed.save(&curr_declared);

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

    /// The reconciler for this filesystem's managed-paths manifest, stored at the manifest filename
    /// under `base_dir` and vetted against `base_dir`.
    fn managed_paths(&self) -> ManagedPaths {
        ManagedPaths::new(
            self.base_dir.join(MANAGED_PATHS_MANIFEST_FILENAME),
            self.base_dir.clone(),
        )
    }

    fn collect_declared_paths(&self) -> HashSet<PathBuf> {
        let mut declared = HashSet::new();
        for (path, entry) in &self.entries {
            entry.collect_declared(path, &mut declared);
        }
        declared
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

/// Error produced while writing the rendered filesystem tree to disk.
#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);

#[cfg(test)]
mod tests {
    use super::*;

    impl FileSystem {
        pub(crate) fn test_empty() -> Self {
            let base_dir = tempfile::tempdir()
                .expect("create temp dir for test FileSystem")
                .keep();
            Self::new(base_dir, HashMap::new())
        }
    }
}
