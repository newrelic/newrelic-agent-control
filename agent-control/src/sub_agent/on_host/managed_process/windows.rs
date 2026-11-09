pub(super) mod process_handle;

use super::spawned_process::error::CommandError;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Instant;
use std::{io, thread};
use tracing::warn;

impl SupervisedProcess {
    pub fn try_wait(self) -> ProcessState {
        match self.process_handle.exit_code() {
            None => ProcessState::Running(self),
            Some(code) => ProcessState::Exited(ExitStatus::from_raw(code)),
        }
    }

    pub fn shutdown(self) -> Result<ExitStatus, CommandError> {
        // TerminateJobObject terminates the process and all processes it has spawned.
        self.job_object.kill().map_err(CommandError::from)?;
        let deadline = Instant::now() + self.shutdown_timeout;
        loop {
            match self.process_handle.exit_code() {
                None => {
                    if Instant::now() >= deadline {
                        warn!(agent_id = %self.agent_id, pid = %self.pid, "Process did not exit after TerminateJobObject within shutdown timeout");
                        break Err(CommandError::IOError(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "process did not exit after TerminateJobObject within shutdown timeout",
                        )));
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                Some(code) => break Ok(ExitStatus::from_raw(code)),
            }
        }
    }
}
