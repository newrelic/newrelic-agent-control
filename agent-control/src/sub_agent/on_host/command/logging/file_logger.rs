//! File logging for executable output: per-agent log files handed to the process as its
//! stdout/stderr.

use crate::agent_control::agent_id::AgentID;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Error produced while opening a sub-agent log file.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct FileLoggerError(String);

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub struct SubAgentFileLoggingConfig {
    pub enabled: bool,
    // Default value is being set by `ConfigPatcher` right after deserialization.
    pub base_path: PathBuf,
}

/// Per-agent log directory: `base_path/<agent_id>`. Shared with `OnHostCleaner` so the path a
/// sub-agent's log files are written to and the path deleted on agent removal never drift apart.
pub fn agent_log_dir(base_path: &Path, agent_id: &AgentID) -> PathBuf {
    base_path.join(agent_id)
}

/// Opens `base_path/<agent_id>/<file_name>` in append mode, creating it if needed, so it can be
/// handed to a spawned process as its stdout or stderr. The process holds its own handle to the
/// file and keeps writing to it independently of Agent Control.
pub fn open_log_file(
    agent_id: &AgentID,
    file_logging_config: &SubAgentFileLoggingConfig,
    file_name: &str,
) -> Result<File, FileLoggerError> {
    let file_dir = agent_log_dir(&file_logging_config.base_path, agent_id);
    DirectoryManagerFs
        .create(&file_dir)
        .map_err(|e| FileLoggerError(format!("creating log directory: {e}")))?;

    let file_path = file_dir.join(file_name);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .map_err(|e| FileLoggerError(format!("opening {}: {e}", file_path.display())))
}
