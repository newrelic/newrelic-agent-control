//! Rendered filesystem tree and the logic to materialize it on disk.
use fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
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
        let manifest_path = self.manifest_path();
        let prev_declared = read_manifest(&manifest_path, &self.base_dir);
        let curr_declared = self.collect_declared_paths();

        // Reconcile: delete paths AC owned previously but no longer declares.
        let mut stale: Vec<&PathBuf> = prev_declared
            .iter()
            .filter(|p| !curr_declared.contains(*p))
            .collect();
        // Deepest first, so a dir's stale children are gone before we remove the dir.
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

/// Reads the managed-paths manifest at `path`, returning only entries that are safe to act on:
/// paths genuinely contained in `base_dir`.
fn read_manifest(path: &Path, base_dir: &Path) -> HashSet<PathBuf> {
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
    let managed_paths = match serde_json::from_slice::<ManagedPathsManifest>(&raw) {
        Ok(m) => m.managed_paths,
        Err(err) => {
            warn!(?err, ?path, "managed-paths manifest is malformed, ignoring");
            return HashSet::new();
        }
    };
    managed_paths
        .into_iter()
        .filter(|p| {
            // Never let the manifest mark itself for deletion.
            if p == path {
                return false;
            }
            let within = is_within_base(p, base_dir);
            if !within {
                warn!(?p, ?base_dir, "An agent is not allowed to modify files outside it's isolated filesystem. Ignoring path.");
            }
            within
        })
        .collect()
}

fn is_within_base(path: &Path, base_dir: &Path) -> bool {
    let has_escape = path.components().any(|c| matches!(c, Component::ParentDir));
    !has_escape && path.is_absolute() && path.starts_with(base_dir)
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
        RenderedEntry::File { content, .. } => write_file(file_writer, dir_manager, path, content),
        RenderedEntry::Dir { children, .. } => {
            ensure_dir(dir_manager, path)?;
            for (sub_path, child) in children {
                let child_path = path.join(sub_path);
                trace!("Recursing into child entry {}", child_path.display());
                write_entry(file_writer, dir_manager, &child_path, child)?;
            }
            Ok(())
        }
        RenderedEntry::DirContentFromMap { files, .. } => {
            ensure_dir(dir_manager, path)?;
            for (file_name, content) in files {
                write_file(file_writer, dir_manager, &path.join(file_name), content)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    impl FileSystem {
        pub(crate) fn test_empty() -> Self {
            let base_dir = tempfile::tempdir()
                .expect("create temp dir for test FileSystem")
                .keep();
            Self::new(base_dir, HashMap::new())
        }
    }

    // `path_from_root` is joined onto an OS-appropriate absolute root when `absolute` is true.
    #[rstest]
    // In-base paths are accepted.
    #[case::file_in_base("base/dir/file.txt", true, true)]
    #[case::nested_in_base("base/dir/a/b/c.txt", true, true)]
    #[case::base_itself("base/dir", true, true)]
    // Outside the base dir.
    #[case::unrelated_absolute("etc/passwd", true, false)]
    #[case::parent_of_base("base", true, false)]
    // Lexical-prefix confusion: a sibling sharing a name prefix must not pass.
    #[case::sibling_prefix("base/dirsuffix/x.txt", true, false)]
    // `..` traversal that resolves outside base but would pass a naive `starts_with`.
    #[case::parent_traversal("base/dir/../escape.txt", true, false)]
    // Relative paths can't be reasoned about safely.
    #[case::relative("relative/path.txt", false, false)]
    fn is_within_base_only_accepts_contained_absolute_paths(
        #[case] path_from_root: &str,
        #[case] absolute: bool,
        #[case] expected: bool,
    ) {
        #[cfg(windows)]
        const ABS_ROOT: &str = "C:\\";
        #[cfg(not(windows))]
        const ABS_ROOT: &str = "/";

        let base = Path::new(ABS_ROOT).join("base").join("dir");
        let path = if absolute {
            Path::new(ABS_ROOT).join(path_from_root)
        } else {
            PathBuf::from(path_from_root)
        };
        assert_eq!(is_within_base(&path, &base), expected);
    }

    #[test]
    fn read_manifest_drops_untrusted_entries() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join(MANAGED_PATHS_MANIFEST_FILENAME);

        let keep_1 = base_dir.join("keep.txt");
        let keep_2 = base_dir.join("sub").join("deep.txt");

        let manifest = ManagedPathsManifest {
            managed_paths: vec![
                keep_1.clone(),
                keep_2.clone(),
                // Out of base: a tampered manifest must not turn these into deletions.
                PathBuf::from("/etc/passwd"),
                base_dir.join("..").join("..").join("escape.txt"),
                PathBuf::from("relative.txt"),
                // The manifest must never list (and thus delete) itself.
                manifest_path.clone(),
            ],
        };
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        let result = read_manifest(&manifest_path, &base_dir);

        assert_eq!(result, HashSet::from([keep_1, keep_2]));
    }

    #[test]
    fn read_manifest_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join(MANAGED_PATHS_MANIFEST_FILENAME);

        assert!(read_manifest(&manifest_path, &base_dir).is_empty());
    }
}
