//! Manifest-based reconcile shared by the rendered on-host filesystems.
//!
//! Agent Control records the absolute paths it wrote on the previous successful write in a JSON
//! manifest, so the next write can delete paths that were "previously managed but are no longer
//! declared". The manifest is read from an agent-writable location, so its entries are vetted
//! against a base directory before any deletion.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use tracing::{trace, warn};

/// On-disk shape of the managed-paths manifest.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ManagedPathsManifest {
    managed_paths: Vec<PathBuf>,
}

impl ManagedPathsManifest {
    /// Loads the manifest at `path`. A missing or malformed file yields an empty manifest (logged).
    fn load(path: &Path) -> Self {
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                warn!(
                    ?err,
                    ?path,
                    "failed to read managed-paths manifest, ignoring"
                );
                return Self::default();
            }
        };
        serde_json::from_slice(&raw).unwrap_or_else(|err| {
            warn!(?err, ?path, "managed-paths manifest is malformed, ignoring");
            Self::default()
        })
    }

    /// Serializes the manifest to `path`, creating its parent directory if needed.
    fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_vec(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        std::fs::write(path, body)
    }
}

/// Reconciles a managed-paths manifest stored at `manifest_path`, vetting its entries against
/// `base_dir`. Reused by every rendered filesystem that needs "delete what we no longer declare".
pub struct ManagedPaths {
    manifest_path: PathBuf,
    base_dir: PathBuf,
}

impl ManagedPaths {
    /// Builds a reconciler for the manifest at `manifest_path`, vetting entries against `base_dir`.
    pub fn new(manifest_path: PathBuf, base_dir: PathBuf) -> Self {
        Self {
            manifest_path,
            base_dir,
        }
    }

    /// Reads the manifest, returning only entries genuinely contained in `base_dir` (and never the
    /// manifest file itself). Untrusted entries are logged and dropped.
    pub fn read(&self) -> HashSet<PathBuf> {
        ManagedPathsManifest::load(&self.manifest_path)
            .managed_paths
            .into_iter()
            .filter(|p| {
                // Never let the manifest mark itself for deletion.
                if p == &self.manifest_path {
                    return false;
                }
                let within = is_within_base(p, &self.base_dir);
                if !within {
                    warn!(?p, base_dir = ?self.base_dir, "An agent is not allowed to modify files outside its isolated filesystem. Ignoring path.");
                }
                within
            })
            .collect()
    }

    /// Deletes paths that were managed previously (`prev`) but are no longer declared (`curr`),
    /// deepest first so a directory's stale children are gone before the directory itself.
    /// Failures are logged and skipped.
    pub fn prune_stale(&self, prev: &HashSet<PathBuf>, curr: &HashSet<PathBuf>) {
        let mut stale: Vec<&PathBuf> = prev.iter().filter(|p| !curr.contains(*p)).collect();
        // Deepest first, so a dir's stale children are gone before we remove the dir.
        stale.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for path in stale {
            if path.exists()
                && let Err(err) = delete_path(path)
            {
                warn!(?err, ?path, "failed to delete stale managed path");
            }
        }
    }

    /// Persists `declared` as the new manifest. Failures are logged and ignored.
    pub fn save(&self, declared: &HashSet<PathBuf>) {
        let mut managed_paths: Vec<PathBuf> = declared.iter().cloned().collect();
        managed_paths.sort();
        let manifest = ManagedPathsManifest { managed_paths };
        if let Err(err) = manifest.save(&self.manifest_path) {
            warn!(
                ?err,
                manifest_path = ?self.manifest_path,
                "failed to persist managed-paths manifest"
            );
        }
    }
}

/// Returns `true` only when `path` is genuinely contained in `base_dir`: an absolute path, with no
/// `..` traversal, that lies under `base_dir`. Used to vet manifest entries before acting on them,
/// since the manifest is read from an agent-writable location and must not be trusted to point
/// outside the agent's own directory.
pub fn is_within_base(path: &Path, base_dir: &Path) -> bool {
    let has_escape = path.components().any(|c| matches!(c, Component::ParentDir));
    !has_escape && path.is_absolute() && path.starts_with(base_dir)
}

/// Recursively deletes the file or directory at `path`.
pub fn delete_path(path: &Path) -> io::Result<()> {
    trace!("Deleting path {}", path.display());
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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
    fn read_drops_untrusted_entries() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join("manifest.json");

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

        let result = ManagedPaths::new(manifest_path, base_dir).read();

        assert_eq!(result, HashSet::from([keep_1, keep_2]));
    }

    #[test]
    fn read_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join("manifest.json");

        assert!(ManagedPaths::new(manifest_path, base_dir).read().is_empty());
    }

    #[test]
    fn prune_stale_deletes_only_undeclared_paths_deepest_first() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join("manifest.json");

        // A nested stale tree (dir + child) plus a declared file that must survive.
        let stale_dir = base_dir.join("stale");
        let stale_child = stale_dir.join("child.txt");
        let kept = base_dir.join("kept.txt");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(&stale_child, "x").unwrap();
        std::fs::write(&kept, "x").unwrap();

        let prev = HashSet::from([stale_dir.clone(), stale_child.clone(), kept.clone()]);
        let curr = HashSet::from([kept.clone()]);

        ManagedPaths::new(manifest_path, base_dir).prune_stale(&prev, &curr);

        assert!(!stale_child.exists(), "stale child should be deleted");
        assert!(!stale_dir.exists(), "stale dir should be deleted");
        assert!(kept.exists(), "declared path must be kept");
    }

    #[test]
    fn save_then_read_round_trips_declared_paths() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().to_path_buf();
        let manifest_path = base_dir.join("manifest.json");
        let managed = ManagedPaths::new(manifest_path, base_dir.clone());

        let declared = HashSet::from([base_dir.join("a.txt"), base_dir.join("b.txt")]);
        managed.save(&declared);

        assert_eq!(managed.read(), declared);
    }
}
