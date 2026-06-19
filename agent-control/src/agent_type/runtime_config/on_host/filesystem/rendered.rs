//! Rendered filesystem tree and the logic to materialize it on disk.
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{trace, warn};

/// Filename of the sidecar manifest Agent Control writes inside each sub-agent's filesystem dir.
/// The manifest records the absolute paths AC wrote on the previous successful write event so
/// the next write can compute "previously managed but no longer declared → delete".
///
/// Reserved name: agent-type definitions cannot declare a path with this exact filename at any
/// level. The filesystem parser validates this at parse time.
pub const MANAGED_PATHS_MANIFEST_FILENAME: &str = ".ac-managed-paths.json";

/// Rendered filesystem tree, ready to be materialized on disk.
///
/// Top-level keys (`entries`) are absolute paths under `base_dir`; children inside a `Dir` are
/// kept relative to their parent — recursion in [`FileSystem::write`] joins them onto the parent
/// path.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem {
    pub(super) base_dir: PathBuf,
    pub(super) entries: HashMap<PathBuf, RenderedEntry>,
}

/// A single rendered filesystem entry.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderedEntry {
    /// A file with the given content.
    File {
        content: String,
        persistent: bool,
    },
    /// A directory containing child entries keyed by their relative path.
    Dir {
        children: HashMap<PathBuf, RenderedEntry>,
        persistent: bool,
    },
    /// A directory whose files were projected from a map (filename to content).
    DirContentFromMap {
        files: HashMap<PathBuf, String>,
        persistent: bool,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ManagedPathsManifest {
    managed_paths: Vec<PathBuf>,
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
        // `FileSystem::default()` produces an empty `base_dir` so we don't want to write a manifest
        if self.base_dir.as_os_str().is_empty() {
            return Ok(());
        }
        let manifest_path = self.manifest_path();
        let prev_declared = read_manifest(&manifest_path);
        let curr_declared = self.collect_declared_paths();

        // Reconcile: delete paths AC owned previously but no longer declares.
        let mut stale: Vec<&PathBuf> = prev_declared
            .iter()
            .filter(|p| !curr_declared.contains(*p))
            .collect();
        // Sort by depth descending so we delete leaves first; saves directory-not-empty churn.
        stale.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for path in stale {
            if path.exists()
                && let Err(err) = delete_path(path)
            {
                warn!(?err, ?path, "failed to delete stale managed path");
            }
        }

        for (path, entry) in &self.entries {
            write_entry(file_writer, dir_manager, path, entry)?;
        }

        if let Err(err) = write_manifest(&manifest_path, &curr_declared) {
            warn!(
                ?err,
                ?manifest_path,
                "failed to persist managed-paths manifest"
            );
        }

        Ok(())
    }

    /// Deletes the on-disk path of every ephemeral entry in the tree.
    /// A persistent entry whose ancestor is ephemeral is wiped along with the ancestor
    pub fn delete_ephemeral(&self) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            delete_ephemeral_recursive(path, entry)?;
        }
        Ok(())
    }

    fn manifest_path(&self) -> PathBuf {
        self.base_dir.join(MANAGED_PATHS_MANIFEST_FILENAME)
    }

    fn collect_declared_paths(&self) -> HashSet<PathBuf> {
        let mut declared = HashSet::new();
        for (path, entry) in &self.entries {
            collect_recursive(path, entry, &mut declared);
        }
        declared
    }
}

fn collect_recursive(path: &Path, entry: &RenderedEntry, declared: &mut HashSet<PathBuf>) {
    declared.insert(path.to_path_buf());
    match entry {
        RenderedEntry::File { .. } => {}
        RenderedEntry::Dir { children, .. } => {
            for (sub, child) in children {
                collect_recursive(&path.join(sub), child, declared);
            }
        }
        RenderedEntry::DirContentFromMap { files, .. } => {
            for sub in files.keys() {
                declared.insert(path.join(sub));
            }
        }
    }
}

fn read_manifest(path: &Path) -> HashSet<PathBuf> {
    let raw = match std::fs::read(path) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return HashSet::new(),
        Err(err) => {
            warn!(
                ?err,
                ?path,
                "failed to read managed-paths manifest, ignoring"
            );
            return HashSet::new();
        }
    };
    match serde_json::from_slice::<ManagedPathsManifest>(&raw) {
        Ok(m) => m.managed_paths.into_iter().collect(),
        Err(err) => {
            warn!(?err, ?path, "managed-paths manifest is malformed, ignoring");
            HashSet::new()
        }
    }
}

fn write_manifest(path: &Path, declared: &HashSet<PathBuf>) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut managed_paths: Vec<PathBuf> = declared.iter().cloned().collect();
    managed_paths.sort();
    let body = serde_json::to_vec(&ManagedPathsManifest { managed_paths })
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, body)
}

fn delete_path(path: &Path) -> Result<(), FileSystemEntriesError> {
    trace!("Deleting stale managed path {}", path.display());
    let res = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    res.map_err(|err| FileSystemEntriesError(format!("deleting {}: {err}", path.display())))
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
        RenderedEntry::DirContentFromMap { files, .. } => {
            dir_manager.create(path).map_err(|err| {
                FileSystemEntriesError(format!("creating directory {path:?}: {err}"))
            })?;
            for (file_name, content) in files {
                let file_path = path.join(file_name);
                let parent = file_path.parent().ok_or_else(|| {
                    FileSystemEntriesError(format!("{} has no parent dir", file_path.display()))
                })?;
                dir_manager.create(parent).map_err(|err| {
                    FileSystemEntriesError(format!("creating directory {parent:?}: {err}"))
                })?;
                file_writer
                    .write(&file_path, content.to_owned())
                    .map_err(|err| {
                        FileSystemEntriesError(format!("creating file {file_path:?}: {err}"))
                    })?;
            }
            Ok(())
        }
    }
}

fn delete_ephemeral_recursive(
    path: &Path,
    entry: &RenderedEntry,
) -> Result<(), FileSystemEntriesError> {
    let persistent = match entry {
        RenderedEntry::File { persistent, .. }
        | RenderedEntry::Dir { persistent, .. }
        | RenderedEntry::DirContentFromMap { persistent, .. } => *persistent,
    };
    if !persistent {
        if path.exists() {
            delete_path(path).inspect_err(|err| warn!(?err, ?path, "delete_ephemeral failed"))?;
        }
        return Ok(());
    }
    // Children may still be ephemeral.
    if let RenderedEntry::Dir { children, .. } = entry {
        for (sub, child) in children {
            delete_ephemeral_recursive(&path.join(sub), child)?;
        }
    }
    Ok(())
}

/// Error produced while writing the rendered filesystem tree to disk.
#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);
