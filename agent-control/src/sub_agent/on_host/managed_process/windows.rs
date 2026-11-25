pub(super) mod process_handle;

use super::spawned_process::error::CommandError;
use super::spawned_process::os_process::OsProcess;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use crate::sub_agent::on_host::process_group_tracker::TrackedProcessGroup;
use process_handle::ProcessHandle;
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::thread;

impl<G: TrackedProcessGroup> SupervisedProcess<G> {
    /// Supervises a process just spawned by this run. The `Child` handle is dropped on return,
    /// the process keeps running and is managed by its pid alone.
    pub(super) fn from_started(os_process: OsProcess<G>) -> Result<Self, CommandError> {
        // `from_os_process` opens a secondary handle while the `Child` is still alive, pinning the
        // process object so the exit code stays retrievable after it is dropped.
        let process_handle = ProcessHandle::from_os_process(&os_process)?;
        Ok(Self {
            pid: os_process.get_pid(),
            agent_id: os_process.agent_id().clone(),
            group: os_process.into_group(),
            process_handle,
        })
    }

    pub fn try_wait(self) -> ProcessState<G> {
        match self.process_handle.exit_code() {
            None => ProcessState::Running(self),
            Some(code) => ProcessState::Exited(ExitStatus::from_raw(code)),
        }
    }

    /// Forcefully terminates the entire process tree via the group and waits for exit.
    pub(super) fn force_kill(self) -> Result<ExitStatus, CommandError> {
        self.group.kill().map_err(CommandError::Tracker)?;
        loop {
            match self.process_handle.exit_code() {
                None => thread::sleep(POLL_INTERVAL),
                Some(code) => break Ok(ExitStatus::from_raw(code)),
            }
        }
    }
}
