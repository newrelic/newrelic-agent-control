//! A process supervised by Agent Control, either spawned by it or adopted from a previous
//! instance, exposing the operations the supervisor needs regardless of its origin.

pub mod spawned_process;

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "windows")]
mod windows;

use crate::agent_control::agent_id::AgentID;
use spawned_process::{error::CommandError, os_process::OsProcessConfig};
use std::process::ExitStatus;
use std::time::Duration;

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;
#[cfg(target_family = "windows")]
use windows::process_handle::ProcessHandle;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The origin of a process that Agent Control will supervise.
///
/// Once [`ProcessOrigin::try_into_supervised`] is called, the result is a [`SupervisedProcess`]
/// ready to be managed regardless of how it was acquired.
// An `Adopt { pid, agent_id, shutdown_timeout }` variant will be added here once per-instance
// bookkeeping (PID files, cgroup inspection) is in place.
pub enum ProcessOrigin {
    /// Start a new process from the given config.
    Spawn(OsProcessConfig),
}

impl ProcessOrigin {
    /// Resolves the origin and returns a [`SupervisedProcess`] ready to be managed.
    pub fn try_into_supervised(self) -> Result<SupervisedProcess, CommandError> {
        match self {
            Self::Spawn(config) => {
                let os_process = config.start()?.stream()?;
                let pid = os_process.get_pid();
                let agent_id = os_process.agent_id().clone();
                let shutdown_timeout = os_process.shutdown_timeout();
                #[cfg(target_family = "windows")]
                let job_object = os_process.create_job_object()?;
                // `from_os_process` opens a secondary handle while OsProcess is still alive,
                // pinning the process object so the exit code stays retrievable after detach.
                #[cfg(target_family = "windows")]
                let process_handle = ProcessHandle::from_os_process(&os_process)?;
                // Detach: drop OsProcess (and its Child handle). The spawned process continues
                // running; from here on both spawned and adopted processes are managed by PID.
                Ok(SupervisedProcess {
                    pid,
                    agent_id,
                    shutdown_timeout,
                    #[cfg(target_family = "windows")]
                    job_object,
                    #[cfg(target_family = "windows")]
                    process_handle,
                })
            }
        }
    }
}

/// A process supervised by Agent Control that has not yet exited.
///
/// Invariant: the underlying OS process is alive. Transitions to [`ExitedProcess`] via
/// [`Self::try_wait`] or [`Self::shutdown`].
pub struct SupervisedProcess {
    pid: u32,
    // Used for logging and the graceful-shutdown timeout on Unix; will also be used on Windows
    // once the adoption path and graceful-shutdown signalling are implemented there.
    #[allow(dead_code)]
    agent_id: AgentID,
    #[allow(dead_code)]
    shutdown_timeout: Duration,
    /// Job Object used to terminate the entire process tree on Windows.
    #[cfg(target_family = "windows")]
    job_object: JobObject,
    /// Keeps the process object alive so the exit code remains retrievable even after the
    /// process exits quickly and before we get a chance to query it.
    #[cfg(target_family = "windows")]
    process_handle: ProcessHandle,
}

impl SupervisedProcess {
    /// Returns the process id.
    pub fn get_pid(&self) -> u32 {
        self.pid
    }

    /// Constructs an [`ExitedProcess`] by consuming `self` and recording `exit_status`.
    fn into_exited(self, exit_status: ExitStatus) -> ExitedProcess {
        ExitedProcess {
            pid: self.pid,
            agent_id: self.agent_id,
            exit_status,
        }
    }
}

/// A process that has exited. Produced by [`SupervisedProcess::try_wait`] or
/// [`SupervisedProcess::shutdown`].
pub struct ExitedProcess {
    #[allow(dead_code)]
    pid: u32,
    #[allow(dead_code)]
    agent_id: AgentID,
    exit_status: ExitStatus,
}

impl ExitedProcess {
    /// Returns the exit status of the process.
    pub fn exit_status(&self) -> ExitStatus {
        self.exit_status
    }
}

/// Result of a non-blocking liveness check.
pub enum ProcessState {
    /// The process is still running; ownership of the [`SupervisedProcess`] is returned.
    Running(SupervisedProcess),
    /// The process has exited.
    Exited(ExitedProcess),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::managed_process::spawned_process::logging::file_logger::SubAgentFileLoggingConfig;
    use crate::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;
    use std::time::Instant;

    /// Blocking wait used only in Windows tests to verify exit-code retrieval via `ProcessHandle`.
    #[cfg(target_family = "windows")]
    impl SupervisedProcess {
        pub(crate) fn wait(self) -> Result<ExitedProcess, spawned_process::error::CommandError> {
            let mut current = self;
            loop {
                match current.try_wait() {
                    ProcessState::Exited(exited) => break Ok(exited),
                    ProcessState::Running(running) => {
                        std::thread::sleep(POLL_INTERVAL);
                        current = running;
                    }
                }
            }
        }
    }

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

        fn spawn(&self) -> (SupervisedProcess, u32) {
            let process = ProcessOrigin::Spawn(OsProcessConfig::new(
                AgentID::AgentControl,
                self,
                SubAgentFileLoggingConfig::default(),
            ))
            .try_into_supervised()
            .unwrap();
            let pid = process.get_pid();
            (process, pid)
        }
    }

    #[test]
    fn spawned_reports_the_pid_of_the_started_command() {
        let (process, pid) = ExecutableData::long_running().spawn();

        assert_eq!(process.get_pid(), pid);

        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_is_running_until_shutdown() {
        let (process, _) = ExecutableData::long_running().spawn();

        let ProcessState::Running(process) = process.try_wait() else {
            panic!("expected process to be running immediately after spawn");
        };

        // shutdown() kills the process, waits for it to exit, and returns an ExitedProcess
        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_wait_returns_the_exit_status() {
        let (process, _) = ExecutableData::short_lived().spawn();

        let exited = process.wait().unwrap();

        assert!(exited.exit_status().success());
    }

    #[test]
    fn spawned_shutdown_returns_the_exit_status() {
        let (process, _) = ExecutableData::short_lived().spawn();

        let exited = process.shutdown().unwrap();

        cfg_select! {
            target_family = "unix" => assert!(!exited.exit_status().success()),
            // TerminateJobObject exits processes with code 0.
            target_family = "windows" => { let _ = exited; },
        }
    }

    #[test]
    fn spawned_exits_naturally() {
        // Verifies that try_wait correctly detects a process that exits on its own.
        let (process, _) = ExecutableData::short_lived().spawn();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut current = process;
        loop {
            match current.try_wait() {
                ProcessState::Exited(_) => return,
                ProcessState::Running(running) => {
                    assert!(Instant::now() < deadline, "process did not exit in time");
                    std::thread::sleep(POLL_INTERVAL);
                    current = running;
                }
            }
        }
    }
}
