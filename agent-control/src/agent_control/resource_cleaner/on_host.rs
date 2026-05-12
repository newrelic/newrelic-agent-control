use std::path::{Path, PathBuf};
use tracing::debug;

use crate::{
    agent_control::{agent_id::AgentID, defaults::AGENT_FILESYSTEM_FOLDER_NAME},
    agent_type::{
        agent_type_id::AgentTypeID,
        runtime_config::on_host::filesystem::rendered::{FilesystemManifest, MANIFEST_FILE_NAME},
    },
};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host resource cleaner that removes an agent's ephemeral filesystem entries upon removal.
///
/// When an agent is removed from the config, its dedicated filesystem directory
/// (`{remote_dir}/filesystem/{agent_id}/`) is cleaned up according to the manifest written
/// there at startup:
/// - Directories listed in the manifest's `persistent_dirs` are left untouched.
/// - Everything else (ephemeral directories and the manifest file itself) is deleted.
/// - If no manifest exists nothing is removed.
pub struct OnHostFilesystemCleaner {
    remote_dir: PathBuf,
}

impl OnHostFilesystemCleaner {
    pub fn new(remote_dir: PathBuf) -> Self {
        Self { remote_dir }
    }
}

impl ResourceCleaner for OnHostFilesystemCleaner {
    fn clean(
        &self,
        agent_id: &AgentID,
        _agent_type: &AgentTypeID,
    ) -> Result<(), ResourceCleanerError> {
        let AgentID::SubAgent(id) = agent_id else {
            return Ok(());
        };

        let agent_fs_dir = self
            .remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(id.as_str());

        if !agent_fs_dir.exists() {
            debug!(%agent_id, "Agent filesystem dir does not exist, skipping cleanup");
            return Ok(());
        }

        let manifest_path = agent_fs_dir.join(MANIFEST_FILE_NAME);
        let Some(persistent_dirs) = read_persistent_dirs(&manifest_path)? else {
            debug!(%agent_id, "No agent filesystem manifest found, skipping cleanup");
            return Ok(());
        };

        if persistent_dirs.is_empty() {
            debug!(%agent_id, "No persistent dirs, removing entire agent filesystem dir");
            return std::fs::remove_dir_all(&agent_fs_dir).map_err(|e| {
                ResourceCleanerError(format!(
                    "removing agent filesystem dir {}: {e}",
                    agent_fs_dir.display()
                ))
            });
        }

        delete_ephemeral_entries(&agent_fs_dir, &persistent_dirs, agent_id)
    }
}

/// Returns `None` when no manifest file is present.
/// Returns `Some(dirs)` when a manifest exists; `dirs` may be empty.
fn read_persistent_dirs(
    manifest_path: &Path,
) -> Result<Option<Vec<PathBuf>>, ResourceCleanerError> {
    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(manifest_path).map_err(|e| {
        ResourceCleanerError(format!(
            "reading filesystem manifest {}: {e}",
            manifest_path.display()
        ))
    })?;

    let manifest: FilesystemManifest = serde_json::from_str(&content).map_err(|e| {
        ResourceCleanerError(format!(
            "parsing filesystem manifest {}: {e}",
            manifest_path.display()
        ))
    })?;

    Ok(Some(manifest.persistent_dirs))
}

/// Deletes all entries in `agent_fs_dir` that are not covered by any persistent dir, including
/// the manifest file itself (since it is no longer needed after cleanup).
fn delete_ephemeral_entries(
    agent_fs_dir: &Path,
    persistent_dirs: &[PathBuf],
    agent_id: &AgentID,
) -> Result<(), ResourceCleanerError> {
    let entries = std::fs::read_dir(agent_fs_dir).map_err(|e| {
        ResourceCleanerError(format!(
            "reading agent filesystem dir {}: {e}",
            agent_fs_dir.display()
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            ResourceCleanerError(format!(
                "reading dir entry in {}: {e}",
                agent_fs_dir.display()
            ))
        })?;
        let entry_path = entry.path();

        // Remove the manifest file along with ephemeral content.
        if entry.file_name() == MANIFEST_FILE_NAME {
            std::fs::remove_file(&entry_path).map_err(|e| {
                ResourceCleanerError(format!("removing manifest {}: {e}", entry_path.display()))
            })?;
            continue;
        }

        // Keep this entry if any persistent dir is rooted inside it.
        // e.g. entry_path = `.../agent-id/var` and persistent = `var/data` → keep `var/`
        let is_persistent_root = persistent_dirs
            .iter()
            .any(|rel| agent_fs_dir.join(rel).starts_with(&entry_path));

        if is_persistent_root {
            debug!(%agent_id, "Keeping persistent root: {}", entry_path.display());
            continue;
        }

        debug!(%agent_id, "Removing ephemeral entry: {}", entry_path.display());
        if entry_path.is_dir() {
            std::fs::remove_dir_all(&entry_path).map_err(|e| {
                ResourceCleanerError(format!("removing dir {}: {e}", entry_path.display()))
            })?;
        } else {
            std::fs::remove_file(&entry_path).map_err(|e| {
                ResourceCleanerError(format!("removing file {}: {e}", entry_path.display()))
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::on_host::filesystem::rendered::FilesystemManifest;
    use tempfile::TempDir;

    fn agent_id(id: &str) -> AgentID {
        AgentID::try_from(id.to_string()).unwrap()
    }

    fn agent_type() -> AgentTypeID {
        AgentTypeID::try_from("ns/test:0.0.1").unwrap()
    }

    fn write_manifest(dir: &Path, persistent_dirs: Vec<PathBuf>) {
        let manifest = FilesystemManifest { persistent_dirs };
        let json = serde_json::to_string(&manifest).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE_NAME), json).unwrap();
    }

    #[test]
    fn no_agent_fs_dir_is_noop() {
        let tmp = TempDir::new().unwrap();
        let cleaner = OnHostFilesystemCleaner::new(tmp.path().to_path_buf());
        let id = agent_id("missing-agent");
        assert!(cleaner.clean(&id, &agent_type()).is_ok());
    }

    #[test]
    fn no_manifest_is_noop() {
        // Filesystems written before this feature have no manifest. The cleaner must leave
        // them untouched to preserve backward compatibility.
        let tmp = TempDir::new().unwrap();
        let remote_dir = tmp.path();

        let id = agent_id("my-agent");
        let AgentID::SubAgent(ref raw_id) = id else {
            panic!()
        };
        let agent_fs = remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(raw_id.as_str());
        std::fs::create_dir_all(agent_fs.join("some/dir")).unwrap();
        std::fs::write(agent_fs.join("some/dir/file.txt"), "hello").unwrap();

        let cleaner = OnHostFilesystemCleaner::new(remote_dir.to_path_buf());
        cleaner.clean(&id, &agent_type()).unwrap();

        assert!(
            agent_fs.join("some/dir/file.txt").exists(),
            "pre-feature filesystem should be left untouched when no manifest is present"
        );
    }

    #[test]
    fn manifest_with_no_persistent_dirs_removes_entire_dir() {
        let tmp = TempDir::new().unwrap();
        let remote_dir = tmp.path();

        let id = agent_id("my-agent");
        let AgentID::SubAgent(ref raw_id) = id else {
            panic!()
        };
        let agent_fs = remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(raw_id.as_str());
        std::fs::create_dir_all(&agent_fs).unwrap();
        write_manifest(&agent_fs, vec![]);

        let cleaner = OnHostFilesystemCleaner::new(remote_dir.to_path_buf());
        cleaner.clean(&id, &agent_type()).unwrap();

        assert!(
            !agent_fs.exists(),
            "agent_fs_dir should be removed when no persistent dirs"
        );
    }

    #[test]
    fn persistent_dirs_are_kept_ephemeral_dirs_removed() {
        let tmp = TempDir::new().unwrap();
        let remote_dir = tmp.path();

        let id = agent_id("my-agent");
        let AgentID::SubAgent(ref raw_id) = id else {
            panic!()
        };
        let agent_fs = remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(raw_id.as_str());

        // Create ephemeral dir with a file
        std::fs::create_dir_all(agent_fs.join("config")).unwrap();
        std::fs::write(agent_fs.join("config/agent.yaml"), "key: val").unwrap();

        // Create persistent dir with a file
        std::fs::create_dir_all(agent_fs.join("data/store")).unwrap();
        std::fs::write(agent_fs.join("data/store/db.json"), "{}").unwrap();

        // Write manifest listing the persistent dir
        write_manifest(&agent_fs, vec![PathBuf::from("data/store")]);

        let cleaner = OnHostFilesystemCleaner::new(remote_dir.to_path_buf());
        cleaner.clean(&id, &agent_type()).unwrap();

        assert!(
            !agent_fs.join("config").exists(),
            "ephemeral 'config' dir should be removed"
        );
        assert!(
            agent_fs.join("data/store/db.json").exists(),
            "persistent 'data/store' contents should be kept"
        );
        assert!(
            !agent_fs.join(MANIFEST_FILE_NAME).exists(),
            "manifest should be removed after cleanup"
        );
    }
}
