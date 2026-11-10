pub(super) mod process_handle;

use super::spawned_process::error::CommandError;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::thread;

impl SupervisedProcess {
    pub fn try_wait(self) -> ProcessState {
        match self.process_handle.exit_code() {
            None => ProcessState::Running(self),
            Some(code) => ProcessState::Exited(ExitStatus::from_raw(code)),
        }
    }

    /// Forcefully terminates the entire process tree via the Job Object and waits for exit.
    pub(super) fn force_kill(self) -> Result<ExitStatus, CommandError> {
        // Destructure to take job_object and process_handle independently: kill(self) consumes
        // job_object, after which the remaining fields no longer form a SupervisedProcess.
        let SupervisedProcess {
            job_object,
            process_handle,
            ..
        } = self;
        job_object.kill().map_err(CommandError::from)?;
        loop {
            match process_handle.exit_code() {
                None => thread::sleep(POLL_INTERVAL),
                Some(code) => break Ok(ExitStatus::from_raw(code)),
            }
        }
    }
}
