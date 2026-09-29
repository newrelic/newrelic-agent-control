//! PoC: on-host bookkeeping of the last-known PID for each spawned sub-agent, so a
//! restarted Agent Control can recognize a still-running process instead of respawning
//! it. Not yet wired into the spawn/adopt path; see the on-host crash-survival CDD/PoC.

use std::io;
use std::path::PathBuf;

use fs::directory_manager::DirectoryManager;
use fs::file::deleter::FileDeleter;
use fs::file::reader::FileReader;
use fs::file::writer::FileWriter;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::FOLDER_NAME_RUNTIME_STATE;

const PROCESS_RECORD_FILE_NAME: &str = "process.yaml";

/// PoC: reads a process's start-time marker (field 22, `starttime`, of `/proc/<pid>/stat`),
/// clock ticks since the current boot, opaque and not meant to survive a host reboot. Used
/// to tell the exact process instance a [`ProcessRecord`] was made for apart from any later
/// process that happens to reuse the same pid.
#[cfg(target_os = "linux")]
pub fn read_proc_start_time_marker(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` (field 2) is parenthesized and may itself contain spaces or parens, so split on
    // the *last* ')' rather than whitespace to find where the numeric fields resume.
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // `state` (field 3) lands at index 0 of `fields`, so `starttime` (field 22) is index 19.
    fields.get(19)?.parse().ok()
}

/// Error reading or writing a [`ProcessRecord`].
#[derive(Debug, Error)]
#[error("process record I/O: {0}")]
pub struct ProcessRecordError(#[from] io::Error);

/// Enough bookkeeping to recognize the same OS process instance after Agent Control
/// restarts, without relying on `wait()`/`try_wait()` (which only work on a real child).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessRecord {
    /// The OS process id at the time Agent Control spawned it.
    pub pid: u32,
    /// An opaque marker distinguishing this exact process instance from any future process
    /// that reuses the same pid, read from `/proc/<pid>/stat` field 22 (starttime) at spawn
    /// time. This is clock ticks since the current boot, not wall-clock time; it isn't
    /// meant to survive a host reboot, only an Agent Control crash/restart within the same
    /// boot session. Cross-checked on restart to reject a reused PID.
    pub start_time_marker: u64,
}

/// Persists and retrieves a [`ProcessRecord`] per agent, keyed by [`AgentID`].
pub trait ProcessRecordStorer {
    /// Stores `record` for `agent_id`, overwriting any existing record.
    fn set(&self, agent_id: &AgentID, record: &ProcessRecord) -> Result<(), ProcessRecordError>;
    /// Retrieves the stored record for `agent_id`, if any.
    fn get(&self, agent_id: &AgentID) -> Result<Option<ProcessRecord>, ProcessRecordError>;
    /// Deletes the stored record for `agent_id`. Deleting a non-existent record is not an error.
    fn delete(&self, agent_id: &AgentID) -> Result<(), ProcessRecordError>;
}

/// [`ProcessRecordStorer`] backed by one YAML file per agent under a `runtime-state`
/// directory, rooted at Agent Control's dynamic data directory.
pub struct FileProcessRecordStorer<F, D> {
    file_rw: F,
    directory_manager: D,
    base_dir: PathBuf,
}

impl<F, D> FileProcessRecordStorer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader + FileDeleter,
{
    /// Creates a storer rooted at `base_dir` (Agent Control's dynamic data directory).
    pub fn new(file_rw: F, directory_manager: D, base_dir: PathBuf) -> Self {
        Self {
            file_rw,
            directory_manager,
            base_dir,
        }
    }

    fn file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.base_dir
            .join(FOLDER_NAME_RUNTIME_STATE)
            .join(agent_id)
            .join(PROCESS_RECORD_FILE_NAME)
    }
}

impl<F, D> ProcessRecordStorer for FileProcessRecordStorer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader + FileDeleter,
{
    fn set(&self, agent_id: &AgentID, record: &ProcessRecord) -> Result<(), ProcessRecordError> {
        let path = self.file_path(agent_id);
        if let Some(parent) = path.parent() {
            self.directory_manager
                .create(parent)
                .map_err(|err| io::Error::other(format!("creating runtime-state dir: {err}")))?;
        }
        let content = serde_saphyr::to_string(record)
            .map_err(|err| io::Error::other(format!("serializing process record: {err}")))?;
        self.file_rw.write(&path, content)?;
        Ok(())
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<ProcessRecord>, ProcessRecordError> {
        let path = self.file_path(agent_id);
        match self.file_rw.read(&path) {
            Ok(content) => {
                let record = serde_saphyr::from_str(&content)
                    .map_err(|err| io::Error::other(format!("parsing process record: {err}")))?;
                Ok(Some(record))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn delete(&self, agent_id: &AgentID) -> Result<(), ProcessRecordError> {
        let path = self.file_path(agent_id);
        match self.file_rw.delete(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;

    fn test_storer(base_dir: PathBuf) -> FileProcessRecordStorer<LocalFile, DirectoryManagerFs> {
        FileProcessRecordStorer::new(LocalFile, DirectoryManagerFs, base_dir)
    }

    #[test]
    fn get_missing_record_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        assert_eq!(storer.get(&agent_id).unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();
        let record = ProcessRecord {
            pid: 4242,
            start_time_marker: 1_700_000_000,
        };

        storer.set(&agent_id, &record).unwrap();

        assert_eq!(storer.get(&agent_id).unwrap(), Some(record));
    }

    #[test]
    fn set_overwrites_previous_record() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid: 2,
                    start_time_marker: 2,
                },
            )
            .unwrap();

        assert_eq!(
            storer.get(&agent_id).unwrap(),
            Some(ProcessRecord {
                pid: 2,
                start_time_marker: 2,
            })
        );
    }

    #[test]
    fn delete_removes_record() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();

        storer.delete(&agent_id).unwrap();

        assert_eq!(storer.get(&agent_id).unwrap(), None);
    }

    #[test]
    fn delete_missing_record_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        storer.delete(&agent_id).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn start_time_marker_is_stable_for_the_same_process() {
        let pid = std::process::id();

        let first = read_proc_start_time_marker(pid);
        let second = read_proc_start_time_marker(pid);

        assert!(first.is_some());
        assert_eq!(first, second);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn start_time_marker_is_none_for_a_nonexistent_pid() {
        // PID 0 is never a real process on Linux.
        assert_eq!(read_proc_start_time_marker(0), None);
    }

    #[test]
    fn records_for_different_agents_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_a = AgentID::try_from("agent-a").unwrap();
        let agent_b = AgentID::try_from("agent-b").unwrap();

        storer
            .set(
                &agent_a,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();

        assert_eq!(storer.get(&agent_b).unwrap(), None);
    }
}
