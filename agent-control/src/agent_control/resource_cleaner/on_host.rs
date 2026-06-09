use std::path::PathBuf;
use tracing::debug;

use crate::{
    agent_control::{agent_id::AgentID, defaults::AGENT_FILESYSTEM_FOLDER_NAME},
    agent_type::agent_type_id::AgentTypeID,
};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host resource cleaner that removes an agent's filesystem directory upon removal.
///
/// When an agent is removed from the config, its entire filesystem directory
/// (`{remote_dir}/filesystem/{agent_id}/`) is deleted. This includes both ephemeral
/// and persistent files.
///
/// Note: Persistent files are only preserved during agent restarts and config updates
/// (handled by startup cleanup in FileSystem::write), not during complete agent removal.
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

        debug!(%agent_id, "Removing agent filesystem directory on agent removal");
        std::fs::remove_dir_all(&agent_fs_dir).map_err(|e| {
            ResourceCleanerError(format!(
                "removing agent filesystem dir {}: {e}",
                agent_fs_dir.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use tempfile::TempDir;

    fn agent_id(id: &str) -> AgentID {
        AgentID::try_from(id.to_string()).unwrap()
    }

    fn agent_type() -> AgentTypeID {
        AgentTypeID::try_from("ns/test:0.0.1").unwrap()
    }

    #[test]
    fn no_agent_fs_dir_is_noop() {
        let tmp = TempDir::new().unwrap();
        let cleaner = OnHostFilesystemCleaner::new(tmp.path().to_path_buf());
        let id = agent_id("missing-agent");
        assert!(cleaner.clean(&id, &agent_type()).is_ok());
    }

    #[test]
    fn removes_entire_filesystem_directory() {
        let tmp = TempDir::new().unwrap();
        let remote_dir = tmp.path();

        let id = agent_id("my-agent");
        let AgentID::SubAgent(ref raw_id) = id else {
            panic!()
        };
        let agent_fs = remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(raw_id.as_str());

        // Create various files and directories
        std::fs::create_dir_all(agent_fs.join("config")).unwrap();
        std::fs::write(agent_fs.join("config/agent.yaml"), "key: val").unwrap();
        std::fs::create_dir_all(agent_fs.join("data/store")).unwrap();
        std::fs::write(agent_fs.join("data/store/db.json"), "{}").unwrap();
        std::fs::write(agent_fs.join("some_file.txt"), "content").unwrap();

        let cleaner = OnHostFilesystemCleaner::new(remote_dir.to_path_buf());
        cleaner.clean(&id, &agent_type()).unwrap();

        // The entire agent filesystem directory should be removed
        assert!(
            !agent_fs.exists(),
            "entire agent filesystem directory should be removed on agent removal"
        );
    }
}
