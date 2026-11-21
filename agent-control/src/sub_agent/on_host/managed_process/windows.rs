pub(super) mod console;
pub(super) mod process_handle;

use super::spawned_process::error::CommandError;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use console::Console;
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

    /// Sends `CTRL_BREAK_EVENT` to the process's console group. Requires the process to have
    /// been spawned with `CREATE_NEW_PROCESS_GROUP` so it is its own group leader.
    pub(super) fn graceful_signal(&self) -> Result<(), CommandError> {
        Console::ensure()
            .send_ctrl_break(self.pid)
            .map_err(CommandError::from)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_agent::on_host::managed_process::ProcessOrigin;
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;
    use std::time::Duration;

    #[test]
    fn graceful_signal_succeeds_when_process_has_exited() {
        let process = ProcessOrigin::Spawn(OsProcessConfig::new(
            "test".to_string().try_into().unwrap(),
            &ExecutableData {
                id: "test".to_string(),
                bin: "cmd".to_string(),
                args: vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
                env: Default::default(),
                restart_policy: Default::default(),
                shutdown_timeout: Default::default(),
            },
            None,
        ))
        .try_into_supervised()
        .unwrap();

        // Wait until the process is gone without consuming it.
        while process.process_handle.exit_code().is_none() {
            thread::sleep(Duration::from_millis(50));
        }

        assert!(process.graceful_signal().is_ok());
    }
}
