pub(super) mod process_handle;

use super::spawned_process::error::CommandError;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;

impl SupervisedProcess {
    pub fn try_wait(self) -> ProcessState {
        match self.process_handle.exit_code() {
            Some(259) => ProcessState::Running(self),
            Some(code) => ProcessState::Exited(self.into_exit_status(ExitStatus::from_raw(code))),
            None => ProcessState::Exited(self.into_exit_status(ExitStatus::from_raw(1))),
        }
    }

    pub fn shutdown(self) -> Result<ExitStatus, CommandError> {
        // Destructure to consume job_object independently of the other fields.
        let SupervisedProcess {
            pid: _,
            agent_id: _,
            shutdown_timeout: _,
            job_object,
            process_handle,
        } = self;
        // TerminateJobObject terminates the process and all processes it has spawned.
        job_object.kill().map_err(CommandError::from)?;
        // Spin-poll using the retained handle until the process reports a final exit code.
        loop {
            match process_handle.exit_code() {
                Some(259) => std::thread::sleep(POLL_INTERVAL),
                Some(code) => return Ok(ExitStatus::from_raw(code)),
                None => return Ok(ExitStatus::from_raw(1)),
            }
        }
    }
}
