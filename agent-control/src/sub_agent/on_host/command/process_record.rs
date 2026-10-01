//! PoC: on-host bookkeeping of the last-known PID for each spawned sub-agent, so a
//! restarted Agent Control can recognize a still-running process instead of respawning
//! it. See `command_os.rs`'s `adoptable_process`/`record_spawned_process` for how this is
//! wired into the spawn/adopt path, and the on-host crash-survival CDD/PoC for the design.

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

/// Reads a process's creation-time marker: opaque, not meant to survive a host reboot, used
/// to tell the exact process instance a [`ProcessRecord`] was made for apart from any later
/// process that happens to reuse the same pid. On Linux: field 22 (`starttime`) of
/// `/proc/<pid>/stat`, clock ticks since the current boot. On Windows: the process creation
/// time from `GetProcessTimes`, a `FILETIME` (100ns ticks since 1601-01-01 UTC). Both are
/// stable for the process's whole lifetime and unique enough across any realistic pid-reuse
/// window; neither needs to survive a reboot, since a reboot kills every supervised process
/// anyway, mooting the whole adoption question.
#[cfg(target_os = "linux")]
pub fn read_process_creation_marker(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` (field 2) is parenthesized and may itself contain spaces or parens, so split on
    // the *last* ')' rather than whitespace to find where the numeric fields resume.
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // `state` (field 3) lands at index 0 of `fields`, so `starttime` (field 22) is index 19.
    fields.get(19)?.parse().ok()
}

/// Windows counterpart of the Linux `read_process_creation_marker` above: opens the process
/// with the minimal `PROCESS_QUERY_LIMITED_INFORMATION` access (sufficient for a same-user
/// process, no `SeDebugPrivilege` needed) and reads its creation `FILETIME` via
/// `GetProcessTimes`, packed into a `u64` the same way the two `u32` halves of a `FILETIME`
/// normally are.
///
/// Checks real liveness first via `GetExitCodeProcess`/`STILL_ACTIVE`, deliberately not just
/// whether `OpenProcess` succeeds: unlike Linux, where a fully-reaped process's
/// `/proc/<pid>/stat` simply stops existing, a Windows process's kernel object (and thus its
/// pid, and `OpenProcess`/`GetProcessTimes` on it) can keep working for a process that has
/// already exited, for as long as *any* handle anywhere — not necessarily held by this
/// code — still references it. Skipping this check cost real test failures: a test's own
/// `Child` handle to an already-killed-and-waited process was enough to keep it adoptable.
#[cfg(target_os = "windows")]
pub fn read_process_creation_marker(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut exit_code = 0u32;
        let still_active = GetExitCodeProcess(handle, &mut exit_code).is_ok()
            && exit_code == STILL_ACTIVE.0 as u32;
        if !still_active {
            let _ = CloseHandle(handle);
            return None;
        }

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }
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
    /// that reuses the same pid, read via [`read_process_creation_marker`] at spawn time
    /// (clock ticks since boot on Linux, a creation `FILETIME` on Windows — see that
    /// function for details). Not meant to survive a host reboot, only an Agent Control
    /// crash/restart within the same boot session. Cross-checked on restart to reject a
    /// reused PID.
    pub start_time_marker: u64,
}

/// Persists and retrieves a [`ProcessRecord`] per executable, keyed by [`AgentID`] *and*
/// executable id. A single agent can declare more than one executable (several on-host
/// scenarios do); keying by `agent_id` alone made every executable under the same agent
/// silently overwrite the others' bookkeeping, since the last one spawned would always win.
pub trait ProcessRecordStorer {
    /// Stores `record` for `(agent_id, exec_id)`, overwriting any existing record.
    fn set(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
        record: &ProcessRecord,
    ) -> Result<(), ProcessRecordError>;
    /// Retrieves the stored record for `(agent_id, exec_id)`, if any.
    fn get(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
    ) -> Result<Option<ProcessRecord>, ProcessRecordError>;
    /// Deletes the stored record for `(agent_id, exec_id)`. Deleting a non-existent record is
    /// not an error.
    fn delete(&self, agent_id: &AgentID, exec_id: &str) -> Result<(), ProcessRecordError>;
}

/// [`ProcessRecordStorer`] backed by one YAML file per `(agent, executable)` pair under a
/// `runtime-state` directory, rooted at Agent Control's dynamic data directory.
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

    fn file_path(&self, agent_id: &AgentID, exec_id: &str) -> PathBuf {
        self.base_dir
            .join(FOLDER_NAME_RUNTIME_STATE)
            .join(agent_id)
            .join(exec_id)
            .join(PROCESS_RECORD_FILE_NAME)
    }
}

impl<F, D> ProcessRecordStorer for FileProcessRecordStorer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader + FileDeleter,
{
    fn set(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
        record: &ProcessRecord,
    ) -> Result<(), ProcessRecordError> {
        let path = self.file_path(agent_id, exec_id);
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

    fn get(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
    ) -> Result<Option<ProcessRecord>, ProcessRecordError> {
        let path = self.file_path(agent_id, exec_id);
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

    fn delete(&self, agent_id: &AgentID, exec_id: &str) -> Result<(), ProcessRecordError> {
        let path = self.file_path(agent_id, exec_id);
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

    const EXEC_ID: &str = "exec-1";

    #[test]
    fn get_missing_record_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        assert_eq!(storer.get(&agent_id, EXEC_ID).unwrap(), None);
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

        storer.set(&agent_id, EXEC_ID, &record).unwrap();

        assert_eq!(storer.get(&agent_id, EXEC_ID).unwrap(), Some(record));
    }

    #[test]
    fn set_overwrites_previous_record() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        storer
            .set(
                &agent_id,
                EXEC_ID,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();
        storer
            .set(
                &agent_id,
                EXEC_ID,
                &ProcessRecord {
                    pid: 2,
                    start_time_marker: 2,
                },
            )
            .unwrap();

        assert_eq!(
            storer.get(&agent_id, EXEC_ID).unwrap(),
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
                EXEC_ID,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();

        storer.delete(&agent_id, EXEC_ID).unwrap();

        assert_eq!(storer.get(&agent_id, EXEC_ID).unwrap(), None);
    }

    #[test]
    fn delete_missing_record_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("test-agent").unwrap();

        storer.delete(&agent_id, EXEC_ID).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn start_time_marker_is_stable_for_the_same_process() {
        let pid = std::process::id();

        let first = read_process_creation_marker(pid);
        let second = read_process_creation_marker(pid);

        assert!(first.is_some());
        assert_eq!(first, second);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn start_time_marker_is_none_for_a_nonexistent_pid() {
        // PID 0 is never a real process on Linux.
        assert_eq!(read_process_creation_marker(0), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn start_time_marker_is_none_for_a_nonexistent_pid() {
        // PID 0 is the "System Idle Process" pseudo-process on Windows; `OpenProcess` on it
        // reliably fails (`ERROR_INVALID_PARAMETER`), making it a deterministic stand-in for
        // "nothing to query" without racing a real process's pid getting reused.
        assert_eq!(read_process_creation_marker(0), None);
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
                EXEC_ID,
                &ProcessRecord {
                    pid: 1,
                    start_time_marker: 1,
                },
            )
            .unwrap();

        assert_eq!(storer.get(&agent_b, EXEC_ID).unwrap(), None);
    }

    /// Regression test: an agent declaring more than one executable (several on-host
    /// scenarios do, e.g. `multiple_executables.rs`, `restarting_processes.rs`) must not have
    /// one executable's bookkeeping clobber another's. Keying by `agent_id` alone used to let
    /// this happen silently: whichever executable spawned last would overwrite the single
    /// shared record, so an earlier executable's own restart could adopt the wrong pid or
    /// find nothing to adopt at all.
    #[test]
    fn records_for_different_executables_under_the_same_agent_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let storer = test_storer(dir.path().to_path_buf());
        let agent_id = AgentID::try_from("multi-exec-agent").unwrap();

        storer
            .set(
                &agent_id,
                "exec-1",
                &ProcessRecord {
                    pid: 111,
                    start_time_marker: 1,
                },
            )
            .unwrap();
        storer
            .set(
                &agent_id,
                "exec-2",
                &ProcessRecord {
                    pid: 222,
                    start_time_marker: 2,
                },
            )
            .unwrap();

        assert_eq!(
            storer.get(&agent_id, "exec-1").unwrap(),
            Some(ProcessRecord {
                pid: 111,
                start_time_marker: 1,
            }),
            "exec-2's record must not have clobbered exec-1's"
        );
        assert_eq!(
            storer.get(&agent_id, "exec-2").unwrap(),
            Some(ProcessRecord {
                pid: 222,
                start_time_marker: 2,
            })
        );
    }
}
