use crate::agent_type::runtime_config::on_host::filesystem::{DirEntriesMap, SafePath};
use ::fs::{directory_manager::DirectoryManager, file::writer::FileWriter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::trace;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem {
    pub(super) ephemeral: HashMap<SafePath, DirEntriesMap>,
    pub(super) persistent: HashMap<SafePath, DirEntriesMap>,
    /// Absolute path to the agent's dedicated filesystem directory
    /// (e.g. `{remote_dir}/filesystem/{agent_id}`).
    pub(super) filesystem_dir: PathBuf,
}

impl FileSystem {
    pub fn write(
        &self,
        file_writer: &impl FileWriter,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        // Clean up ephemeral entries from previous runs before writing new ones.
        // This ensures stale files are removed even if agent-control was down when the config changed.
        self.cleanup_ephemeral_on_startup(dir_manager)?;

        write_ephemeral_dirs(&self.ephemeral, file_writer, dir_manager)?;
        write_persistent_dirs(&self.persistent, file_writer, dir_manager)?;

        Ok(())
    }

    /// Cleans up ephemeral filesystem entries from a previous run before writing new ones.
    ///
    /// Compares what's on disk against the current agent type's filesystem configuration
    /// (`self.ephemeral` and `self.persistent`) and deletes any directories that don't match.
    /// This ensures:
    /// - Old ephemeral directories that are no longer in the config are removed
    /// - Persistent directories that are still in the config are preserved
    ///
    /// This is a no-op if the filesystem directory doesn't exist yet (first run).
    fn cleanup_ephemeral_on_startup(
        &self,
        dir_manager: &impl DirectoryManager,
    ) -> Result<(), FileSystemEntriesError> {
        if self.filesystem_dir.as_os_str().is_empty() || !self.filesystem_dir.exists() {
            return Ok(());
        }

        // Build a set of all directories that should exist based on the current config
        let should_exist: std::collections::HashSet<PathBuf> = self
            .ephemeral
            .keys()
            .chain(self.persistent.keys())
            .map(|safe_path| safe_path.as_ref().to_path_buf())
            .collect();

        // If no directories are configured, we can delete the entire filesystem dir
        if should_exist.is_empty() {
            trace!(
                "No filesystem directories configured, removing entire filesystem dir: {}",
                self.filesystem_dir.display()
            );
            dir_manager.delete(&self.filesystem_dir).map_err(|e| {
                FileSystemEntriesError(format!(
                    "removing filesystem dir on startup {}: {e}",
                    self.filesystem_dir.display()
                ))
            })?;
            return Ok(());
        }

        // Delete directories that exist on disk but are not in the current config
        delete_stale_directories(&self.filesystem_dir, &should_exist, dir_manager)
    }
}

/// Clears each directory before writing its contents.
/// This ensures files no longer present in the config are removed from disk.
fn write_ephemeral_dirs(
    dirs: &HashMap<SafePath, DirEntriesMap>,
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    dirs.iter().try_for_each(|(dir_path, dir_entries)| {
        // Delete existing contents so stale files (e.g. removed integrations) are cleaned up.
        // `delete` is a no-op when the directory does not yet exist.
        dir_manager.delete(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("clearing ephemeral directory {dir_path:?}: {err}"))
        })?;
        dir_manager.create(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("creating directory {dir_path:?}: {err}"))
        })?;
        dir_entries
            .0
            .iter()
            .try_for_each(|(sub_path, file_content)| {
                let file_path = dir_path.as_ref().join(sub_path);
                create_file(file_writer, dir_manager, &file_path, file_content)
            })
    })
}

/// Writes directory contents additively — existing files are overwritten but never deleted.
/// Used for persistent directories whose contents must survive config updates.
fn write_persistent_dirs(
    dirs: &HashMap<SafePath, DirEntriesMap>,
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    dirs.iter().try_for_each(|(dir_path, dir_entries)| {
        dir_manager.create(dir_path.as_ref()).map_err(|err| {
            FileSystemEntriesError(format!("creating directory {dir_path:?}: {err}"))
        })?;
        dir_entries
            .0
            .iter()
            .try_for_each(|(sub_path, file_content)| {
                let file_path = dir_path.as_ref().join(sub_path);
                create_file(file_writer, dir_manager, &file_path, file_content)
            })
    })
}

fn create_file(
    file_writer: &impl FileWriter,
    dir_manager: &impl DirectoryManager,
    file_path: &PathBuf,
    file_content: &String,
) -> Result<(), FileSystemEntriesError> {
    trace!("Writing filesystem entry to {}", file_path.display());
    let parent_dir = file_path.parent().ok_or_else(|| {
        FileSystemEntriesError(format!("{} has no parent dir", file_path.display()))
    })?;
    dir_manager.create(parent_dir).map_err(|err| {
        FileSystemEntriesError(format!("creating directory {parent_dir:?}: {err}"))
    })?;
    // Will overwrite files if they already exist!
    file_writer
        .write(file_path.as_path(), file_content.to_owned())
        .map_err(|err| FileSystemEntriesError(format!("creating file {file_path:?}: {err}")))
}

/// Deletes directories in `filesystem_dir` that are not in the `should_exist` set.
/// This is used during startup cleanup to remove stale directories based on the current
/// agent type configuration.
fn delete_stale_directories(
    filesystem_dir: &Path,
    should_exist: &std::collections::HashSet<PathBuf>,
    dir_manager: &impl DirectoryManager,
) -> Result<(), FileSystemEntriesError> {
    let entries = std::fs::read_dir(filesystem_dir).map_err(|e| {
        FileSystemEntriesError(format!(
            "reading filesystem dir {}: {e}",
            filesystem_dir.display()
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            FileSystemEntriesError(format!(
                "reading dir entry in {}: {e}",
                filesystem_dir.display()
            ))
        })?;
        let entry_path = entry.path();

        // Keep this directory if it's in the current config (ephemeral or persistent)
        if should_exist.contains(&entry_path) {
            trace!("Keeping configured directory: {}", entry_path.display());
            continue;
        }

        // Keep this directory if it's a parent of any configured directory
        // e.g., keep "data" if "data/store" is configured
        let is_parent_of_configured = should_exist
            .iter()
            .any(|path| path.starts_with(&entry_path));

        if is_parent_of_configured {
            trace!(
                "Keeping parent directory of configured path: {}",
                entry_path.display()
            );
            continue;
        }

        // This directory is stale - not in the current config
        trace!(
            "Removing stale directory on startup: {}",
            entry_path.display()
        );
        dir_manager.delete(&entry_path).map_err(|e| {
            FileSystemEntriesError(format!(
                "removing stale directory {}: {e}",
                entry_path.display()
            ))
        })?;
    }

    Ok(())
}

#[derive(Debug, Error)]
#[error("file system entries error: {0}")]
pub struct FileSystemEntriesError(String);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::runtime_config::on_host::filesystem::SafePath;
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Test that ephemeral directories are cleaned up on startup before writing new files.
    /// This simulates the scenario where agent-control was shut down, files were left on disk,
    /// and then agent-control starts up again. The old ephemeral files should be removed,
    /// but persistent files should be kept.
    #[test]
    fn cleanup_ephemeral_on_startup() {
        let tmp_dir = TempDir::new().unwrap();
        let filesystem_dir = tmp_dir.path().to_path_buf();

        // Step 1: Create initial filesystem with both ephemeral and persistent dirs
        let initial_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("config")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("agent.yaml")),
                    "initial config".to_string(),
                )])),
            )]),
            persistent: HashMap::from([(
                SafePath(filesystem_dir.join("data")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("state.json")),
                    "initial state".to_string(),
                )])),
            )]),
            filesystem_dir: filesystem_dir.clone(),
        };

        // Write the initial filesystem
        initial_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write initial filesystem");

        // Verify initial state: both ephemeral and persistent files exist
        assert!(
            filesystem_dir.join("config/agent.yaml").exists(),
            "Initial ephemeral file should exist"
        );
        assert!(
            filesystem_dir.join("data/state.json").exists(),
            "Initial persistent file should exist"
        );

        // Step 2: Simulate a filesystem with different ephemeral content but same persistent dirs.
        // When this is written, the old ephemeral files should be cleaned up.
        let new_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("integrations")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("integration.yaml")),
                    "new integration".to_string(),
                )])),
            )]),
            persistent: HashMap::from([(
                SafePath(filesystem_dir.join("data")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("state.json")),
                    "updated state".to_string(),
                )])),
            )]),
            filesystem_dir: filesystem_dir.clone(),
        };

        // Write the new filesystem
        new_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write new filesystem");

        // Verify: old ephemeral dir (config/) should be removed
        assert!(
            !filesystem_dir.join("config").exists(),
            "Old ephemeral dir 'config' should be removed on startup cleanup"
        );
        assert!(
            !filesystem_dir.join("config/agent.yaml").exists(),
            "Old ephemeral file should be removed"
        );

        // Verify: new ephemeral dir should exist
        assert!(
            filesystem_dir
                .join("integrations/integration.yaml")
                .exists(),
            "New ephemeral file should exist"
        );

        // Verify: persistent dir and its contents should survive
        assert!(
            filesystem_dir.join("data/state.json").exists(),
            "Persistent file should survive startup cleanup"
        );

        // Verify: the content of persistent file should be updated
        let persistent_content =
            std::fs::read_to_string(filesystem_dir.join("data/state.json")).unwrap();
        assert_eq!(
            persistent_content, "updated state",
            "Persistent file content should be updated"
        );
    }

    /// Test that cleanup removes directories not in the current agent type configuration.
    #[test]
    fn cleanup_removes_stale_dirs_not_in_config() {
        let tmp_dir = TempDir::new().unwrap();
        let filesystem_dir = tmp_dir.path().to_path_buf();

        // Create an old directory on disk (simulating a previous run or manual creation)
        std::fs::create_dir_all(filesystem_dir.join("old_dir")).unwrap();
        std::fs::write(filesystem_dir.join("old_dir/old_file.txt"), "old content").unwrap();

        // Create a new filesystem with different directories
        let new_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("new_dir")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("new_file.txt")),
                    "new content".to_string(),
                )])),
            )]),
            persistent: HashMap::default(),
            filesystem_dir: filesystem_dir.clone(),
        };

        // Write the new filesystem - should clean up old_dir since it's not in the config
        new_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write new filesystem");

        // Old directory should be removed (not in current agent type config)
        assert!(
            !filesystem_dir.join("old_dir").exists(),
            "Stale directory should be removed when not in current config"
        );

        // New files should exist
        assert!(
            filesystem_dir.join("new_dir/new_file.txt").exists(),
            "New files should be created"
        );
    }

    /// Test that parent directories of configured nested paths are preserved during cleanup.
    /// For example, if "data/store" is configured, the "data" parent directory should be kept
    /// during startup cleanup (it won't be deleted even though it's not directly in the config).
    #[test]
    fn cleanup_preserves_parent_directories() {
        let tmp_dir = TempDir::new().unwrap();
        let filesystem_dir = tmp_dir.path().to_path_buf();

        // Step 1: Create initial filesystem with a top-level directory
        let initial_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("cache")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("temp.json")),
                    "cache data".to_string(),
                )])),
            )]),
            persistent: HashMap::default(),
            filesystem_dir: filesystem_dir.clone(),
        };

        initial_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write initial filesystem");

        assert!(
            filesystem_dir.join("cache/temp.json").exists(),
            "Initial cache file should exist"
        );

        // Step 2: Change config to use a nested path "data/store" instead of "cache"
        let new_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("data/store")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("items.json")),
                    "store data".to_string(),
                )])),
            )]),
            persistent: HashMap::default(),
            filesystem_dir: filesystem_dir.clone(),
        };

        new_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write new filesystem");

        // The old "cache" directory should be removed (not in new config)
        assert!(
            !filesystem_dir.join("cache").exists(),
            "Old cache directory should be removed during cleanup"
        );

        // The new nested path should exist, with its parent directory preserved
        assert!(
            filesystem_dir.join("data").exists(),
            "Parent directory 'data' should exist"
        );
        assert!(
            filesystem_dir.join("data/store/items.json").exists(),
            "Nested path should be created"
        );
    }

    /// Test cleanup when all dirs are ephemeral (no persistent dirs).
    /// The entire filesystem dir should be removed and recreated.
    #[test]
    fn cleanup_removes_all_when_no_persistent_dirs() {
        let tmp_dir = TempDir::new().unwrap();
        let filesystem_dir = tmp_dir.path().to_path_buf();

        // Step 1: Create initial filesystem with only ephemeral dirs
        let initial_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("config")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("agent.yaml")),
                    "config".to_string(),
                )])),
            )]),
            persistent: HashMap::default(),
            filesystem_dir: filesystem_dir.clone(),
        };

        initial_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write initial filesystem");

        assert!(filesystem_dir.join("config/agent.yaml").exists());

        // Step 2: Write a new filesystem with different ephemeral content
        let new_fs = FileSystem {
            ephemeral: HashMap::from([(
                SafePath(filesystem_dir.join("integrations")),
                DirEntriesMap(HashMap::from([(
                    SafePath(PathBuf::from("integration.yaml")),
                    "integration".to_string(),
                )])),
            )]),
            persistent: HashMap::default(),
            filesystem_dir: filesystem_dir.clone(),
        };

        new_fs
            .write(&LocalFile, &DirectoryManagerFs)
            .expect("Failed to write new filesystem");

        // Old ephemeral dir should be gone
        assert!(
            !filesystem_dir.join("config").exists(),
            "Old ephemeral dir should be removed"
        );

        // New ephemeral dir should exist
        assert!(
            filesystem_dir
                .join("integrations/integration.yaml")
                .exists(),
            "New ephemeral dir should exist"
        );
    }
}
