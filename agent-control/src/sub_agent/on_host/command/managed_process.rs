//! A process supervised by Agent Control, either spawned by it or adopted from a previous
//! instance, exposing the operations the supervisor needs regardless of its origin.

use super::{command_os::CommandOSStarted, error::CommandError};
use crate::agent_control::agent_id::AgentID;
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tracing::warn;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A process supervised by Agent Control.
// Only one value exists per running executable, so the size difference between variants is irrelevant.
#[allow(clippy::large_enum_variant)]
pub enum ManagedProcess {
    /// Spawned by this Agent Control instance, which is therefore its real parent.
    Spawned(CommandOSStarted),
    /// Already running when this instance started. Agent Control is not its parent.
    // Nothing constructs it until process adoption is implemented.
    #[allow(dead_code)]
    Adopted(AdoptedProcess),
}

/// A running process Agent Control did not spawn.
// Placeholder: the behavior is pending the adoption design.
#[allow(dead_code)]
pub struct AdoptedProcess {
    agent_id: AgentID,
    pid: u32,
    /// Platform-dependent data to recognize the exact process instance.
    additional_data: u64,
    shutdown_timeout: Duration,
}

impl ManagedProcess {
    /// Returns the process id.
    pub fn get_pid(&self) -> u32 {
        match self {
            Self::Spawned(started) => started.get_pid(),
            Self::Adopted(adopted) => adopted.pid,
        }
    }

    /// Returns whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        match self {
            Self::Spawned(started) => started.is_running(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Waits for the process to exit and releases its resources.
    pub(crate) fn wait(self) -> Result<ExitStatus, CommandError> {
        match self {
            Self::Spawned(started) => started.wait(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    /// Stops the process, attempting a graceful shutdown and forcing a kill after the timeout.
    pub fn shutdown(&mut self) -> Result<(), CommandError> {
        // Attempt a graceful shutdown (platform-dependent).
        let graceful_shutdown_result = self.graceful_shutdown();

        if let Err(e) = &graceful_shutdown_result {
            warn!(agent_id = %self.agent_id(), "Graceful shutdown failed for process {}: {e}", self.get_pid());
        }

        if graceful_shutdown_result.is_err()
            || self.is_running_after_timeout(self.shutdown_timeout())
        {
            self.kill()?;
        }

        Ok(())
    }

    fn agent_id(&self) -> &AgentID {
        match self {
            Self::Spawned(started) => started.agent_id(),
            Self::Adopted(adopted) => &adopted.agent_id,
        }
    }

    fn shutdown_timeout(&self) -> Duration {
        match self {
            Self::Spawned(started) => started.shutdown_timeout(),
            Self::Adopted(adopted) => adopted.shutdown_timeout,
        }
    }

    fn graceful_shutdown(&mut self) -> Result<(), CommandError> {
        match self {
            Self::Spawned(started) => started.graceful_shutdown(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    fn kill(&mut self) -> Result<(), CommandError> {
        match self {
            Self::Spawned(started) => started.kill(),
            Self::Adopted(_) => unimplemented!("adopted processes are not supported yet"),
        }
    }

    fn is_running_after_timeout(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if !self.is_running() {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_agent::on_host::command::command_os::CommandOSNotStarted;
    use crate::sub_agent::on_host::command::executable_data::ExecutableData;
    use crate::sub_agent::on_host::command::logging::file_logger::SubAgentFileLoggingConfig;

    #[cfg(target_family = "unix")]
    fn long_running() -> ExecutableData {
        ExecutableData::new("test".to_owned(), "sleep".to_owned()).with_args(vec!["30".to_owned()])
    }

    #[cfg(target_family = "windows")]
    fn long_running() -> ExecutableData {
        ExecutableData::new("test".to_owned(), "ping".to_owned()).with_args(vec![
            "-n".to_owned(),
            "30".to_owned(),
            "127.0.0.1".to_owned(),
        ])
    }

    #[cfg(target_family = "unix")]
    fn short_lived() -> ExecutableData {
        ExecutableData::new("test".to_owned(), "true".to_owned())
    }

    #[cfg(target_family = "windows")]
    fn short_lived() -> ExecutableData {
        ExecutableData::new("test".to_owned(), "cmd".to_owned()).with_args(vec![
            "/C".to_owned(),
            "exit".to_owned(),
            "0".to_owned(),
        ])
    }

    fn spawn(executable_data: &ExecutableData) -> (ManagedProcess, u32) {
        let started = CommandOSNotStarted::new(
            AgentID::AgentControl,
            executable_data,
            SubAgentFileLoggingConfig::default(),
        )
        .start()
        .unwrap();
        let pid = started.get_pid();
        (ManagedProcess::Spawned(started), pid)
    }

    #[test]
    fn spawned_reports_the_pid_of_the_started_command() {
        let (mut process, pid) = spawn(&long_running());

        assert_eq!(process.get_pid(), pid);

        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_is_running_until_shutdown() {
        let (mut process, _) = spawn(&long_running());
        assert!(process.is_running());

        process.shutdown().unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        while process.is_running() {
            assert!(Instant::now() < deadline, "process did not stop in time");
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    #[test]
    fn spawned_wait_returns_the_exit_status() {
        let (process, _) = spawn(&short_lived());

        let status = process.wait().unwrap();

        assert!(status.success());
    }

    #[test]
    fn spawned_wait_after_shutdown_reports_unsuccessful_status() {
        let (mut process, _) = spawn(&long_running());

        process.shutdown().unwrap();

        assert!(!process.wait().unwrap().success());
    }

    #[test]
    fn spawned_shutdown_of_an_exited_process_is_not_an_error() {
        let (mut process, _) = spawn(&short_lived());
        let deadline = Instant::now() + Duration::from_secs(10);
        while process.is_running() {
            assert!(Instant::now() < deadline, "process did not exit in time");
            std::thread::sleep(POLL_INTERVAL);
        }

        // On Unix the graceful signal reaches a reaped-pending zombie or fails and falls back
        // to `kill`, which is harmless on an exited child.
        assert!(process.shutdown().is_ok());
    }
}
