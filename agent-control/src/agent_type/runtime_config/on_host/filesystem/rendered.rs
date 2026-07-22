//! Rendered filesystem tree and the logic to materialize it on disk.
use fs::file::copier::FileCopier;
use fs::file::deleter::FileDeleter;
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::trace;

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
    },
    /// A directory containing child entries keyed by their relative path.
    Dir {
        /// The dictionary containing each children path and the entry.
        children: HashMap<PathBuf, RenderedEntry>,
    },
    /// A directory whose files were projected from a map (filename to content).
    DirContentFromMap {
        /// The dictionary containing all file paths and their content.
        files: HashMap<PathBuf, String>,
    },
}

impl RenderedEntry {
    /// Materializes this entry (and its subtree) on disk at `path`. `file_ops` provides the write,
    /// copy and delete capabilities (a single type, [`LocalFile`], implements all three in
    /// production). Delete is needed to clear a stale entry whose on-disk shape doesn't match what
    /// is declared here anymore, e.g. after an Agent Type update changes a path's `kind` between
    /// `file` and `dir`/`dir_content_from_map`.
    fn write(
        &self,
        path: &Path,
        file_ops: &(impl FileWriter + FileCopier + FileDeleter),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        match self {
            Self::File { content, .. } => match content {
                FileContent::Text(text) => write_file(file_ops, dir_manager, path, text),
                FileContent::Copy(source) => copy_file(file_ops, dir_manager, path, source),
            },
            Self::Dir { children, .. } => {
                ensure_dir(file_ops, dir_manager, path)?;
                for (sub_path, child) in children {
                    let child_path = path.join(sub_path);
                    trace!("Recursing into child entry {}", child_path.display());
                    child.write(&child_path, file_ops, dir_manager)?;
                }
                Ok(())
            }
            Self::DirContentFromMap { files, .. } => {
                // Without this clear, a remote cfg that removes a file from the map would leave
                // the old file on disk until the agent control is stopped.
                delete_path(path, file_ops, dir_manager)
                    .map_err(|err| FileSystemEntriesError(format!("clearing {path:?}: {err}")))?;
                ensure_dir(file_ops, dir_manager, path)?;
                for (file_name, content) in files {
                    write_file(file_ops, dir_manager, &path.join(file_name), content)?;
                }
                Ok(())
            }
        }
    }
}

impl FileSystem {
    pub(super) fn new(entries: HashMap<PathBuf, RenderedEntry>) -> Self {
        Self { entries }
    }

    /// Writes the declared tree under `base_dir`, overwriting any declared paths already on disk.
    pub fn write(
        &self,
        file_ops: &(impl FileWriter + FileCopier + FileDeleter),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.write(path, file_ops, dir_manager)?;
        }
        Ok(())
    }

    /// Deletes the on-disk path of every top-level entry that is not declared. Never descends into
    /// a declared `dir`'s own contents: those are left to the agent (or an earlier agent-type
    /// version) to own, and are only re-rendered by `write`, not pruned.
    pub fn delete_not_declared(
        &self,
        file_ops: &impl FileDeleter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        let base_dir = match self.entries.keys().next().and_then(|p| p.parent()) {
            Some(p) => p.to_path_buf(),
            None => return Ok(()),
        };
        // Top-level entry keys are absolute; strip to filename so they can be compared against
        // the single-component names `dir_manager.list` returns.
        let declared: HashSet<PathBuf> = self
            .entries
            .keys()
            .filter_map(|abs| abs.file_name().map(PathBuf::from))
            .collect();
        // list() returns an empty vec when the directory does not exist — no extra NotFound handling needed.
        let children = dir_manager
            .list(&base_dir)
            .map_err(|e| FileSystemEntriesError(format!("listing {}: {e}", base_dir.display())))?;
        for abs in children {
            let name = PathBuf::from(abs.file_name().unwrap_or_default());
            if !declared.contains(&name) {
                delete_path(&abs, file_ops, dir_manager).map_err(|e| {
                    FileSystemEntriesError(format!("deleting {}: {e}", abs.display()))
                })?;
            }
        }
        Ok(())
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
        file_ops: &(impl FileWriter + FileCopier + FileDeleter),
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        for (path, entry) in &self.entries {
            entry.write(path, file_ops, dir_manager)?;
        }
        Ok(())
    }
}

/// Creates `dir` (and any missing parents), with error context. Safe if it already exists. Clears
/// a stale file left at `dir` first (e.g. by a previous Agent Type declaring it as `kind: file`).
fn ensure_dir(
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
    dir: &Path,
) -> Result<(), FileSystemEntriesError> {
    clear_if_wrong_shape(dir, true, file_ops, dir_manager)
        .map_err(|err| FileSystemEntriesError(format!("clearing {dir:?}: {err}")))?;
    trace!("Creating directory {}", dir.display());
    dir_manager
        .create(dir)
        .map_err(|err| FileSystemEntriesError(format!("creating directory {dir:?}: {err}")))
}

/// Writes `content` to `path`, creating its parent directory first. Overwrites an existing file.
/// Clears a stale directory left at `path` first (e.g. by a previous Agent Type declaring it as
/// `kind: dir` or `dir_content_from_map`).
fn write_file(
    file_ops: &(impl FileWriter + FileDeleter),
    dir_manager: &impl DirectoryManager,
    path: &Path,
    content: &str,
) -> Result<(), FileSystemEntriesError> {
    trace!("Writing filesystem entry to {}", path.display());
    // We ensure the parent exists even if the dir is declared independently.
    let parent = path
        .parent()
        .ok_or_else(|| FileSystemEntriesError(format!("{} has no parent dir", path.display())))?;
    ensure_dir(file_ops, dir_manager, parent)?;
    clear_if_wrong_shape(path, false, file_ops, dir_manager)
        .map_err(|err| FileSystemEntriesError(format!("clearing {path:?}: {err}")))?;
    file_ops
        .write(path, content.to_owned())
        .map_err(|err| FileSystemEntriesError(format!("creating file {path:?}: {err}")))
}

/// Copies `source` to `path`, creating its parent directory first. Overwrites an existing file.
/// Clears a stale directory left at `path` first, for the same reason as [`write_file`].
fn copy_file(
    file_ops: &(impl FileCopier + FileDeleter),
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
    ensure_dir(file_ops, dir_manager, parent)?;
    clear_if_wrong_shape(path, false, file_ops, dir_manager)
        .map_err(|err| FileSystemEntriesError(format!("clearing {path:?}: {err}")))?;
    file_ops
        .copy(source, path)
        .map_err(|err| FileSystemEntriesError(format!("copying {source:?} to {path:?}: {err}")))
}

/// Removes whatever currently occupies `path` if it exists and its on-disk shape (directory vs.
/// file) doesn't match `expect_dir`. A missing path is not an error.
fn clear_if_wrong_shape(
    path: &Path,
    expect_dir: bool,
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
) -> std::io::Result<()> {
    if path.exists() && path.is_dir() != expect_dir {
        delete_path(path, file_ops, dir_manager)?;
    }
    Ok(())
}

/// Deletes the file or directory at `path`. A missing path is not an error.
fn delete_path(
    path: &Path,
    file_ops: &impl FileDeleter,
    dir_manager: &impl DirectoryManager,
) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
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

    fn file_entry_with_text(text: &str) -> RenderedEntry {
        RenderedEntry::File {
            content: FileContent::Text(text.into()),
        }
    }

    fn file_entry_copy(source: PathBuf) -> RenderedEntry {
        RenderedEntry::File {
            content: FileContent::Copy(source),
        }
    }

    fn dir_entry(children: HashMap<PathBuf, RenderedEntry>) -> RenderedEntry {
        RenderedEntry::Dir { children }
    }

    fn map_dir_entry(files: HashMap<PathBuf, String>) -> RenderedEntry {
        RenderedEntry::DirContentFromMap { files }
    }

    #[test]
    fn write_creates_file_with_text_content_and_missing_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let target_dir = tmp.path().join("does-not-exist-yet");
        let target_path = target_dir.join("file.txt");

        let fs = FileSystem::new(HashMap::from([(
            target_path.clone(),
            file_entry_with_text("hello"),
        )]));
        fs.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert!(target_dir.is_dir(), "missing parent dir must be created");
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "hello");
    }

    #[test]
    fn write_copies_file_content_from_source() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source.bin");
        let source_bytes = [0xFFu8, 0x00, b'b', b'i', b'n'];
        std::fs::write(&source, source_bytes).unwrap();

        let target_path = tmp.path().join("does-not-exist-yet").join("dest.bin");
        let fs = FileSystem::new(HashMap::from([(
            target_path.clone(),
            file_entry_copy(source),
        )]));
        fs.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert_eq!(std::fs::read(&target_path).unwrap(), source_bytes);
    }

    #[test]
    fn write_creates_dir_and_recurses_into_children() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let fs = FileSystem::new(HashMap::from([(
            base.join("parent"),
            dir_entry(HashMap::from([
                (PathBuf::from("child.txt"), file_entry_with_text("child")),
                (PathBuf::from("empty-nested"), dir_entry(HashMap::new())),
            ])),
        )]));
        fs.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert_eq!(
            std::fs::read_to_string(base.join("parent/child.txt")).unwrap(),
            "child"
        );
        assert!(base.join("parent/empty-nested").is_dir());
    }

    #[test]
    fn write_dir_content_from_map_creates_files_from_map() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let fs = FileSystem::new(HashMap::from([(
            base.join("logging.d"),
            map_dir_entry(HashMap::from([
                (PathBuf::from("a.yaml"), "a-content".to_string()),
                (PathBuf::from("b.yaml"), "b-content".to_string()),
            ])),
        )]));
        fs.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert_eq!(
            std::fs::read_to_string(base.join("logging.d/a.yaml")).unwrap(),
            "a-content"
        );
        assert_eq!(
            std::fs::read_to_string(base.join("logging.d/b.yaml")).unwrap(),
            "b-content"
        );
    }

    #[test]
    fn write_dir_content_from_map_creates_dir_when_map_is_empty() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let fs = FileSystem::new(HashMap::from([(
            base.join("logging.d"),
            map_dir_entry(HashMap::new()),
        )]));
        fs.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert!(base.join("logging.d").is_dir());
    }

    #[test]
    fn write_dir_content_from_map_clears_directory_before_rewrite() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let first = FileSystem::new(HashMap::from([(
            base.join("logging.d"),
            map_dir_entry(HashMap::from([(
                PathBuf::from("a.yaml"),
                "a-content".to_string(),
            )])),
        )]));
        first.write(&LocalFile, &DirectoryManagerFs).unwrap();
        // Content that lands in the directory by other means (e.g. the agent process) is not
        // tracked by the map, but is still cleared: the whole directory is wiped before rewrite.
        std::fs::write(base.join("logging.d/untracked.txt"), "stray").unwrap();

        let second = FileSystem::new(HashMap::from([(
            base.join("logging.d"),
            map_dir_entry(HashMap::from([(
                PathBuf::from("b.yaml"),
                "b-content".to_string(),
            )])),
        )]));
        second.write(&LocalFile, &DirectoryManagerFs).unwrap();

        assert!(
            !base.join("logging.d/a.yaml").exists(),
            "key dropped from the map must be gone"
        );
        assert!(
            !base.join("logging.d/untracked.txt").exists(),
            "untracked content must be gone: the directory is cleared before rewrite"
        );
        assert_eq!(
            std::fs::read_to_string(base.join("logging.d/b.yaml")).unwrap(),
            "b-content"
        );
    }

    /// Writing a `File` entry at a path that already holds different content overwrites it.
    #[test]
    fn write_overwrites_previously_written_file_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("file.txt");

        FileSystem::new(HashMap::from([(path.clone(), file_entry_with_text("v1"))]))
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();
        FileSystem::new(HashMap::from([(path.clone(), file_entry_with_text("v2"))]))
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2");
    }

    /// Writing a `kind: file` entry clears a stale directory left there by a previous Agent Type.
    #[test]
    fn write_replaces_stale_directory_with_declared_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("leftover.txt"), "old").unwrap();

        FileSystem::new(HashMap::from([(
            path.clone(),
            file_entry_with_text("hello"),
        )]))
        .write(&LocalFile, &DirectoryManagerFs)
        .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    /// Writing a `kind: dir` entry clears a stale file left there by a previous Agent Type.
    #[test]
    fn write_replaces_stale_file_with_declared_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        std::fs::write(&path, "old").unwrap();

        FileSystem::new(HashMap::from([(
            path.clone(),
            dir_entry(HashMap::from([(
                PathBuf::from("child.txt"),
                file_entry_with_text("child"),
            )])),
        )]))
        .write(&LocalFile, &DirectoryManagerFs)
        .unwrap();

        assert!(path.is_dir());
        assert_eq!(
            std::fs::read_to_string(path.join("child.txt")).unwrap(),
            "child"
        );
    }

    /// Writing a `dir_content_from_map` entry clears a stale file left there by a previous Agent Type.
    #[test]
    fn write_dir_content_from_map_replaces_stale_file_with_declared_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("entry");
        std::fs::write(&path, "old").unwrap();

        FileSystem::new(HashMap::from([(
            path.clone(),
            map_dir_entry(HashMap::from([(
                PathBuf::from("a.yaml"),
                "a-content".to_string(),
            )])),
        )]))
        .write(&LocalFile, &DirectoryManagerFs)
        .unwrap();

        assert!(path.is_dir());
        assert_eq!(
            std::fs::read_to_string(path.join("a.yaml")).unwrap(),
            "a-content"
        );
    }

    /// Writing a `Dir` entry that already exists correctly must not touch its existing contents.
    #[test]
    fn write_dir_does_not_delete_existing_directory_contents_when_shape_already_matches() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("managed-dir");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("untracked.txt"), "untracked").unwrap();

        FileSystem::new(HashMap::from([(
            path.clone(),
            dir_entry(HashMap::from([(
                PathBuf::from("declared.txt"),
                file_entry_with_text("declared"),
            )])),
        )]))
        .write(&LocalFile, &DirectoryManagerFs)
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(path.join("untracked.txt")).unwrap(),
            "untracked",
            "untracked content in an already-correct directory must survive"
        );
        assert_eq!(
            std::fs::read_to_string(path.join("declared.txt")).unwrap(),
            "declared"
        );
    }

    #[test]
    fn delete_not_declared_removes_undeclared() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        std::fs::write(base.join("declared.yaml"), "d").unwrap();
        std::fs::write(base.join("undeclared.yaml"), "u").unwrap();

        std::fs::create_dir(base.join("declared-dir")).unwrap();
        std::fs::write(base.join("declared-dir/inner.txt"), "inner").unwrap();

        std::fs::create_dir(base.join("undeclared-dir")).unwrap();
        std::fs::write(base.join("undeclared-dir/inner.txt"), "inner").unwrap();

        let fs = FileSystem::new(HashMap::from([
            (base.join("declared.yaml"), file_entry_with_text("x")),
            (base.join("declared-dir"), dir_entry(HashMap::new())),
        ]));

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
        assert!(
            base.join("declared-dir/inner.txt").exists(),
            "a file inside a declared dir must be kept, even though it isn't itself declared"
        );
        assert!(
            !base.join("undeclared-dir").exists(),
            "an undeclared dir, and everything inside it, must be deleted"
        );
    }

    /// DirContentFromMap dirs contents are skipped by delete_not_declared, write() owns their cleanup.
    #[test]
    fn delete_not_declared_skips_map_dir_contents() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        std::fs::create_dir(base.join("logging.d")).unwrap();
        std::fs::write(base.join("logging.d/syslog.yaml"), "sys").unwrap();
        std::fs::write(base.join("logging.d/stale.yaml"), "stale").unwrap();

        let fs = FileSystem::new(HashMap::from([(
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
            base.join("logging.d/stale.yaml").exists(),
            "stale map file is not touched by delete_not_declared; write() cleans it up"
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
