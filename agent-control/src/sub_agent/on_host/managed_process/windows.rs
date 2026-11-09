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
        self.job_object.kill().map_err(CommandError::from)?;
        let mut current = self;
        loop {
            match current.try_wait() {
                ProcessState::Exited(exit_status) => break Ok(exit_status),
                ProcessState::Running(running) => {
                    thread::sleep(POLL_INTERVAL);
                    current = running;
                }
            }
        }
    }
}
