//! Per-agent log files that on-host executables write their stdout/stderr to.

use super::error::CommandError;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_FILE_NAME, STDOUT_LOG_FILE_NAME};
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Whether sub-agents write their stdout/stderr to log files, and the directory holding the
/// per-agent log directories.
#[derive(Debug, PartialEq, Clone)]
pub struct SubAgentFileLoggingConfig {
    pub enabled: bool,
    pub base_path: PathBuf,
}

/// Per-agent log directory: `base_path/<agent_id>`. Shared with `OnHostCleaner` so the path a
/// sub-agent's log files are written to and the path deleted on agent removal never drift apart.
pub fn agent_log_dir(base_path: &Path, agent_id: &AgentID) -> PathBuf {
    base_path.join(agent_id)
}

pub(crate) fn std_streams(maybe_log_dir: &Option<PathBuf>) -> Result<(Stdio, Stdio), CommandError> {
    let Some(log_dir) = maybe_log_dir else {
        return Ok((Stdio::null(), Stdio::null()));
    };
    let (stdout, stderr) = open_std_log_files(log_dir)?;
    Ok((stdout.into(), stderr.into()))
}

/// Creates `log_dir` if needed and opens the stdout and stderr log files inside it in append mode,
/// so they can be handed to a spawned process. The process holds its own handles to the files and
/// keeps writing to them independently of Agent Control.
fn open_std_log_files(log_dir: &Path) -> Result<(File, File), CommandError> {
    DirectoryManagerFs.create(log_dir).map_err(|e| {
        CommandError::LogFile(format!("creating directory {}: {e}", log_dir.display()))
    })?;

    let stdout = open_append(&log_dir.join(STDOUT_LOG_FILE_NAME))?;
    let stderr = open_append(&log_dir.join(STDERR_LOG_FILE_NAME))?;
    Ok((stdout, stderr))
}

fn open_append(log_dir: &Path) -> Result<File, CommandError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir)
        .map_err(|e| CommandError::LogFile(format!("opening {}: {e}", log_dir.display())))
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::sub_agent::on_host::managed_process::spawned_process::log_files::SubAgentFileLoggingConfig;
    use assert_matches::assert_matches;
    use std::io::Write;
    use std::path::PathBuf;

    impl SubAgentFileLoggingConfig {
        pub fn disabled() -> Self {
            Self {
                enabled: false,
                base_path: PathBuf::default(),
            }
        }
    }

    #[test]
    fn open_std_log_files_creates_missing_dir_and_both_files() {
        let base = tempfile::tempdir().unwrap();
        let log_dir = base.path().join("not").join("yet").join("there");

        open_std_log_files(&log_dir).unwrap();

        assert!(log_dir.join(STDOUT_LOG_FILE_NAME).is_file());
        assert!(log_dir.join(STDERR_LOG_FILE_NAME).is_file());
    }

    #[test]
    fn open_std_log_files_appends_to_existing_content() {
        let log_dir = tempfile::tempdir().unwrap();
        let stdout_path = log_dir.path().join(STDOUT_LOG_FILE_NAME);
        std::fs::write(&stdout_path, "previous\n").unwrap();

        let (mut stdout, _stderr) = open_std_log_files(log_dir.path()).unwrap();
        stdout.write_all(b"current\n").unwrap();

        assert_eq!(
            std::fs::read_to_string(&stdout_path).unwrap(),
            "previous\ncurrent\n"
        );
    }

    #[test]
    fn open_std_log_files_fails_when_dir_cannot_be_created() {
        let base = tempfile::tempdir().unwrap();
        let file = base.path().join("a-file");
        std::fs::write(&file, "").unwrap();

        let err = open_std_log_files(&file.join("child")).unwrap_err();

        assert_matches!(err, CommandError::LogFile(msg) if msg.contains("creating directory"));
    }
}
