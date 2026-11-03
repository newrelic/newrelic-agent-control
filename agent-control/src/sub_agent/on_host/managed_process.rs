//! A process supervised by Agent Control, either spawned by it or adopted from a previous
//! instance, exposing the operations the supervisor needs regardless of its origin.

pub mod adopted_process;
pub mod spawned_process;

use adopted_process::AdoptedProcess;
use spawned_process::{error::CommandError, os_process::OsProcess};

use crate::agent_control::agent_id::AgentID;
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tracing::warn;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A process supervised by Agent Control.
#[allow(clippy::large_enum_variant)] // `AdoptedProcess` isn't yet stable, so this warning might be transient.
pub enum ManagedProcess {
    /// Spawned by this Agent Control instance, which is therefore its real parent.
    Spawned(OsProcess),
    /// Already running when this instance started. Agent Control is not its parent.
    #[allow(dead_code)] // Nothing constructs it until process adoption is implemented.
    Adopted(AdoptedProcess),
}

impl ManagedProcess {
    /// Returns the process id.
    pub fn get_pid(&self) -> u32 {
        match self {
            Self::Spawned(started) => started.get_pid(),
            Self::Adopted(adopted) => adopted.get_pid(),
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

    /// Stops the process, attempting a graceful shutdown and forcing a kill if the process is still running after the timeout.
    pub fn shutdown(&mut self) -> Result<(), CommandError> {
        match self.graceful_shutdown() {
            Err(error) => {
                warn!(agent_id = %self.agent_id(), pid =  %self.get_pid(), %error, "Graceful shutdown failed. Stopping forcefully");
                self.kill()
            }
            Ok(()) if self.is_running_after_timeout(self.shutdown_timeout()) => {
                warn!(agent_id = %self.agent_id(), pid =  %self.get_pid(), "Graceful shutdown timeout exceeded. Stopping forcefully");
                self.kill()
            }
            Ok(()) => Ok(()),
        }
    }

    fn agent_id(&self) -> &AgentID {
        match self {
            Self::Spawned(started) => started.agent_id(),
            Self::Adopted(adopted) => adopted.agent_id(),
        }
    }

    fn shutdown_timeout(&self) -> Duration {
        match self {
            Self::Spawned(started) => started.shutdown_timeout(),
            Self::Adopted(adopted) => adopted.shutdown_timeout(),
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
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::managed_process::spawned_process::logging::file_logger::SubAgentFileLoggingConfig;
    use crate::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;

    impl ExecutableData {
        fn long_running() -> Self {
            cfg_select! {
                target_family = "unix" => Self::new("test".to_owned(), "sleep".to_owned()).with_args(vec!["30".to_owned()]),
                target_family = "windows" => {
                    Self::new("test".to_owned(), "ping".to_owned()).with_args(vec![
                        "-n".to_owned(),
                        "30".to_owned(),
                        "127.0.0.1".to_owned(),
                    ])
                }
            }
        }

        fn short_lived() -> Self {
            cfg_select! {
                target_family = "unix" => Self::new("test".to_owned(), "true".to_owned()),
                target_family = "windows" => {
                    Self::new("test".to_owned(), "cmd".to_owned()).with_args(vec![
                        "/C".to_owned(),
                        "exit".to_owned(),
                        "0".to_owned(),
                    ])
                },
            }
        }

        fn spawn(&self) -> (ManagedProcess, u32) {
            let started = OsProcessConfig::new(
                AgentID::AgentControl,
                self,
                SubAgentFileLoggingConfig::default(),
            )
            .start()
            .unwrap();
            let pid = started.get_pid();
            (ManagedProcess::Spawned(started), pid)
        }
    }

    #[test]
    fn spawned_reports_the_pid_of_the_started_command() {
        let (mut process, pid) = ExecutableData::long_running().spawn();

        assert_eq!(process.get_pid(), pid);

        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_is_running_until_shutdown() {
        let (mut process, _) = ExecutableData::long_running().spawn();
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
        let (process, _) = ExecutableData::short_lived().spawn();

        let status = process.wait().unwrap();

        assert!(status.success());
    }

    #[test]
    fn spawned_wait_after_shutdown_returns_the_exit_status() {
        let (mut process, _) = ExecutableData::short_lived().spawn();

        process.shutdown().unwrap();

        let status = process.wait().unwrap();

        cfg_select! {
            target_family = "unix" => assert!(!status.success()),
            // The Windows job object terminates its processes with exit code 0.
            target_family = "windows" => { let _ = status; },
        }
    }

    #[test]
    fn spawned_shutdown_of_an_exited_process_is_not_an_error() {
        let (mut process, _) = ExecutableData::short_lived().spawn();
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
