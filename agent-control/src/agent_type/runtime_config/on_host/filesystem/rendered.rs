//! Rendered filesystem tree and the logic to materialize it on disk.
use fs::file::copier::FileCopier;
use fs::file::deleter::FileDeleter;
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::HashMap;
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
    fn delete_ephemeral(
        &self,
        path: &Path,
        file_ops: &impl FileDeleter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        if !self.persistent() {
            if path.exists() {
                let result = match self {
                    Self::File { .. } => file_ops.delete(path),
                    _ => dir_manager.delete(path),
                };
                result
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
                child.delete_ephemeral(&path.join(sub), file_ops, dir_manager)?;
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
    pub fn delete_ephemeral(
        &self,
        file_ops: &impl FileDeleter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.delete_ephemeral(path, file_ops, dir_manager)?;
        }
        Ok(())
    }

    /// Deletes the on-disk path of every entry in the filesystem that is not declared and is not a
    /// child of a persistent declared directory.
    ///
    /// # Known limitation — previously-declared entries inside persistent directories
    ///
    /// Persistent directories are skipped entirely so that runtime files the agent wrote (and
    /// never declared) are preserved. The unavoidable side-effect is that a file which *was*
    /// declared inside a persistent directory in a previous type version but is no longer declared
    /// in the current one also survives. We would need to save the state to handle this case
    pub fn delete_not_declared(
        &self,
        file_ops: &impl FileDeleter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        let base_dir = match self.entries.keys().next().and_then(|p| p.parent()) {
            Some(p) => p.to_path_buf(),
            None => return Ok(()),
        };
        // Top-level entry keys are absolute; strip to filename so the recursive helper
        // can look items up by their single-component relative name at each level.
        let declared: HashMap<PathBuf, &RenderedEntry> = self
            .entries
            .iter()
            .filter_map(|(abs, entry)| abs.file_name().map(|n| (PathBuf::from(n), entry)))
            .collect();
        prune_undeclared(&base_dir, &declared, file_ops, dir_manager)
    }
}

/// Rendered shared filesystem tree, materialized under the base shared across sub-agents.
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

/// Walks `dir` on disk and deletes every item whose single-component name is absent from
/// `declared`. Persistent `Dir` entries are skipped entirely — their contents are agent-managed.
/// Non-persistent `Dir` entries are recursed into. `DirContentFromMap` dirs are cleaned of files
/// not present in the rendered map.
fn prune_undeclared(
    dir: &Path,
    declared: &HashMap<PathBuf, &RenderedEntry>,
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    // list() returns an empty vec when the directory does not exist — no extra NotFound handling needed.
    let children = dir_manager
        .list(dir)
        .map_err(|e| FileSystemEntriesError(format!("listing {}: {e}", dir.display())))?;
    for abs in children {
        let name = PathBuf::from(abs.file_name().unwrap_or_default());
        match declared.get(&name) {
            None => {
                delete_path(&abs, file_ops, dir_manager).map_err(|e| {
                    FileSystemEntriesError(format!("deleting {}: {e}", abs.display()))
                })?;
            }
            Some(entry) => match *entry {
                RenderedEntry::File { .. } => {}
                RenderedEntry::Dir {
                    persistent: true, ..
                } => {}
                RenderedEntry::Dir { children, .. } => {
                    let child_refs: HashMap<PathBuf, &RenderedEntry> =
                        children.iter().map(|(k, v)| (k.clone(), v)).collect();
                    prune_undeclared(&abs, &child_refs, file_ops, dir_manager)?;
                }
                RenderedEntry::DirContentFromMap { files } => {
                    prune_undeclared_in_map_dir(&abs, files, file_ops, dir_manager)?;
                }
            },
        }
    }
    Ok(())
}

/// Deletes files inside a `DirContentFromMap` directory that are not present in the rendered map.
fn prune_undeclared_in_map_dir(
    dir: &Path,
    declared_files: &HashMap<PathBuf, String>,
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    let children = dir_manager
        .list(dir)
        .map_err(|e| FileSystemEntriesError(format!("listing {}: {e}", dir.display())))?;
    for abs in children {
        let name = PathBuf::from(abs.file_name().unwrap_or_default());
        if !declared_files.contains_key(&name) {
            delete_path(&abs, file_ops, dir_manager)
                .map_err(|e| FileSystemEntriesError(format!("deleting {}: {e}", abs.display())))?;
        }
    }
    Ok(())
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
/// Deletes the file or directory at `path`.
fn delete_path(
    path: &Path,
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
) -> std::io::Result<()> {
    trace!("Deleting path {}", path.display());
    if path.is_dir() {
        dir_manager.delete(path)
    } else {
        file_ops.delete(path)
    }
}

/// Error produced while writing the rendered filesystem tree to disk.
#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);

#[cfg(test)]
mod tests {
    use super::*;
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;
    use tempfile::TempDir;

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

    fn file_entry(persistent: bool) -> RenderedEntry {
        RenderedEntry::File {
            content: FileContent::Text("x".into()),
            persistent,
        }
    }

    fn dir_entry(persistent: bool, children: HashMap<PathBuf, RenderedEntry>) -> RenderedEntry {
        RenderedEntry::Dir {
            children,
            persistent,
        }
    }

    fn map_dir_entry(files: HashMap<PathBuf, String>) -> RenderedEntry {
        RenderedEntry::DirContentFromMap { files }
    }

    fn fs_with(entries: HashMap<PathBuf, RenderedEntry>) -> FileSystem {
        FileSystem::new(entries)
    }

    /// Undeclared top-level file is deleted; declared file is kept.
    #[test]
    fn delete_not_declared_removes_undeclared_top_level_file() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        std::fs::write(base.join("declared.yaml"), "d").unwrap();
        std::fs::write(base.join("undeclared.yaml"), "u").unwrap();

        let fs = fs_with(HashMap::from([(
            base.join("declared.yaml"),
            file_entry(false),
        )]));

        fs.delete_not_declared(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            base.join("declared.yaml").exists(),
            "declared file must be kept"
        );
        assert!(
            !base.join("undeclared.yaml").exists(),
            "undeclared file must be deleted"
        );
    }

    /// Undeclared files inside a non-persistent declared dir are deleted; declared children kept.
    #[test]
    fn delete_not_declared_recurses_into_non_persistent_dir() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        std::fs::create_dir(base.join("config")).unwrap();
        std::fs::write(base.join("config/declared.yaml"), "d").unwrap();
        std::fs::write(base.join("config/runtime.log"), "r").unwrap();

        let fs = fs_with(HashMap::from([(
            base.join("config"),
            dir_entry(
                false,
                HashMap::from([(PathBuf::from("declared.yaml"), file_entry(false))]),
            ),
        )]));

        fs.delete_not_declared(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            base.join("config/declared.yaml").exists(),
            "declared child must be kept"
        );
        assert!(
            !base.join("config/runtime.log").exists(),
            "undeclared child must be deleted"
        );
    }

    /// Nothing inside a persistent declared dir is touched, even if undeclared.
    #[test]
    fn delete_not_declared_skips_persistent_dir_contents() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        std::fs::create_dir(base.join("data")).unwrap();
        std::fs::write(base.join("data/agent-state.db"), "state").unwrap();
        std::fs::write(base.join("data/undeclared-but-inside-persistent.txt"), "x").unwrap();

        let fs = fs_with(HashMap::from([(
            base.join("data"),
            dir_entry(true, HashMap::new()),
        )]));

        fs.delete_not_declared(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            base.join("data/agent-state.db").exists(),
            "contents of persistent dir must be kept"
        );
        assert!(
            base.join("data/undeclared-but-inside-persistent.txt")
                .exists(),
            "undeclared files inside persistent dir must be kept"
        );
    }

    /// Files inside a DirContentFromMap dir that are absent from the rendered map are deleted.
    #[test]
    fn delete_not_declared_cleans_undeclared_files_in_map_dir() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        std::fs::create_dir(base.join("logging.d")).unwrap();
        std::fs::write(base.join("logging.d/syslog.yaml"), "sys").unwrap();
        std::fs::write(base.join("logging.d/stale.yaml"), "stale").unwrap();

        let fs = fs_with(HashMap::from([(
            base.join("logging.d"),
            map_dir_entry(HashMap::from([(
                PathBuf::from("syslog.yaml"),
                "sys".into(),
            )])),
        )]));

        fs.delete_not_declared(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert!(
            base.join("logging.d/syslog.yaml").exists(),
            "declared map file must be kept"
        );
        assert!(
            !base.join("logging.d/stale.yaml").exists(),
            "undeclared map file must be deleted"
        );
    }

    /// No-op when the filesystem has no declared entries.
    #[test]
    fn delete_not_declared_is_noop_when_entries_empty() {
        FileSystem::test_empty()
            .delete_not_declared(&LocalFile, &DirectoryManagerFs)
            .unwrap();
    }
}
