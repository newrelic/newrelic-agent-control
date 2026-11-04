//! Per-agent log files that on-host executables write their stdout/stderr to.

use super::error::CommandError;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_FILE_NAME, STDOUT_LOG_FILE_NAME};
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Per-agent log directory: `base_path/<agent_id>`. Shared with `OnHostCleaner` so the path a
/// sub-agent's log files are written to and the path deleted on agent removal never drift apart.
pub fn agent_log_dir(base_path: &Path, agent_id: &AgentID) -> PathBuf {
    base_path.join(agent_id)
}

/// Creates `log_dir` if needed and opens the stdout and stderr log files inside it in append mode,
/// so they can be handed to a spawned process. The process holds its own handles to the files and
/// keeps writing to them independently of Agent Control.
pub(crate) fn open_std_log_files(log_dir: &Path) -> Result<(File, File), CommandError> {
    DirectoryManagerFs.create(log_dir).map_err(|e| {
        CommandError::LogFile(format!("creating directory {}: {e}", log_dir.display()))
    })?;

    let stdout = open_append(&log_dir.join(STDOUT_LOG_FILE_NAME))?;
    let stderr = open_append(&log_dir.join(STDERR_LOG_FILE_NAME))?;
    Ok((stdout, stderr))
}

fn open_append(path: &Path) -> Result<File, CommandError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| CommandError::LogFile(format!("opening {}: {e}", path.display())))
}
