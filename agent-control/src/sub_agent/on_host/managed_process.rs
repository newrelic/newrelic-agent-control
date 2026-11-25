//! A process supervised by Agent Control, exposing the operations the supervisor needs.

pub mod spawned_process;

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "windows")]
mod windows;

use crate::agent_control::agent_id::AgentID;
use crate::sub_agent::on_host::process_group_tracker::TrackedProcessGroup;
use spawned_process::{error::CommandError, os_process::OsProcessConfig};
use std::process::ExitStatus;
#[cfg(target_family = "unix")]
use std::thread;
use std::time::Duration;
#[cfg(target_family = "unix")]
use std::time::Instant;
use tracing::warn;

#[cfg(target_family = "windows")]
use windows::process_handle::ProcessHandle;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A process supervised by Agent Control that has not yet exited.
///
/// Invariant: the underlying OS process is alive. Transitions to an exit status via
/// [`Self::try_wait`] or [`Self::shutdown`].
pub struct SupervisedProcess<G> {
    pid: u32,
    agent_id: AgentID,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
    /// Tracked process group the process runs in, killed when the process ends.
    group: G,
    /// Keeps the process object alive so the exit code remains retrievable even after the
    /// process exits quickly and before we get a chance to query it.
    #[cfg(target_family = "windows")]
    process_handle: ProcessHandle,
}

/// Result of a non-blocking liveness check.
pub enum ProcessState<G> {
    /// The process is still running; ownership of the [`SupervisedProcess`] is returned.
    Running(SupervisedProcess<G>),
    /// The process has exited.
    Exited(ExitStatus),
}

impl<G: TrackedProcessGroup> SupervisedProcess<G> {
    /// Starts a new process from the config and supervises it.
    pub fn spawn(config: OsProcessConfig<G>) -> Result<Self, CommandError> {
        // TODO OsProcess abstraction doesn't seem to help much, remove it in the future.
        config.start().and_then(Self::from_started)
    }

    pub fn shutdown(self) -> Result<ExitStatus, CommandError> {
        cfg_select! {
            target_family = "windows" => {
                // On Windows there is no direct equivalent to sending SIGTERM. Applications that run as
                // services handle stop signals via Service Control Manager (SCM), and console applications
                // can handle Ctrl-C or Ctrl-Break events via attached consoles.
                // The current implementation uses Job Objects to manage process groups, and there is no
                // graceful shutdown signal sent to the processes.
                warn!(agent_id = %self.agent_id, pid = self.pid, "No graceful shutdown on Windows, stopping forcefully");
                self.force_kill()
            }
            _ => {
                let agent_id = self.agent_id.clone();
                let pid = self.pid;
                let shutdown_timeout = self.shutdown_timeout;

                if let Err(e) = self.graceful_signal() {
                    warn!(agent_id = %agent_id, pid, %e, "Graceful shutdown failed, stopping forcefully");
                    return self.force_kill();
                }

                let deadline = Instant::now() + shutdown_timeout;
                let mut current = self;
                loop {
                    match current.try_wait() {
                        ProcessState::Exited(exit_status) => break Ok(exit_status),
                        ProcessState::Running(running) => {
                            if Instant::now() >= deadline {
                                warn!(agent_id = %agent_id, pid, "Graceful shutdown timeout exceeded, stopping forcefully");
                                break running.force_kill();
                            }
                            thread::sleep(POLL_INTERVAL);
                            current = running;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::process_group_tracker::tests::{
        TestProcessGroup, test_process_config,
    };
    use std::time::Instant;

    /// Blocking wait used only in Windows tests to verify exit-code retrieval via `ProcessHandle`.
    #[cfg(target_family = "windows")]
    impl SupervisedProcess<TestProcessGroup> {
        pub(crate) fn wait(self) -> Result<ExitStatus, spawned_process::error::CommandError> {
            let mut current = self;
            loop {
                match current.try_wait() {
                    ProcessState::Exited(exit_status) => break Ok(exit_status),
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

        fn spawn(&self) -> (SupervisedProcess<TestProcessGroup>, u32) {
            let process =
                SupervisedProcess::spawn(test_process_config(AgentID::AgentControl, self, None))
                    .unwrap();
            let pid = process.pid;
            (process, pid)
        }
    }

    #[test]
    fn spawned_reports_the_pid_of_the_started_command() {
        let (process, pid) = ExecutableData::long_running().spawn();

        assert_eq!(process.pid, pid);

        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_is_running_until_shutdown() {
        let (process, _) = ExecutableData::long_running().spawn();

        let ProcessState::Running(process) = process.try_wait() else {
            panic!("expected process to be running immediately after spawn");
        };

        process.shutdown().unwrap();
    }

    #[test]
    fn spawned_wait_returns_the_exit_status() {
        let (process, _) = ExecutableData::short_lived().spawn();

        let exited = process.wait().unwrap();

        assert!(exited.success());
    }

    #[test]
    fn spawned_shutdown_returns_the_exit_status() {
        let (process, _) = ExecutableData::short_lived().spawn();

        let exited = process.shutdown().unwrap();

        cfg_select! {
            target_family = "unix" => assert!(!exited.success()),
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

    #[cfg(target_family = "unix")]
    mod grouped {
        use super::*;
        use crate::sub_agent::on_host::process_group_tracker::ProcessGroupTracker;
        use crate::sub_agent::on_host::process_group_tracker::tests::{
            CountingProcessGroup, CountingProcessGroupTracker,
        };

        fn spawn_in_group(
            executable: &ExecutableData,
            tracker: &CountingProcessGroupTracker,
        ) -> SupervisedProcess<CountingProcessGroup> {
            let group = tracker
                .group(&AgentID::AgentControl, &executable.id)
                .unwrap();
            SupervisedProcess::spawn(OsProcessConfig::new(
                AgentID::AgentControl,
                executable,
                None,
                group,
            ))
            .unwrap()
        }

        fn wait_for_exit(process: SupervisedProcess<CountingProcessGroup>) -> ExitStatus {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut current = process;
            loop {
                match current.try_wait() {
                    ProcessState::Exited(exit_status) => break exit_status,
                    ProcessState::Running(running) => {
                        assert!(Instant::now() < deadline, "process did not exit in time");
                        std::thread::sleep(POLL_INTERVAL);
                        current = running;
                    }
                }
            }
        }

        #[test]
        fn natural_exit_kills_the_group_once() {
            let tracker = CountingProcessGroupTracker::default();
            let process = spawn_in_group(&ExecutableData::short_lived(), &tracker);

            let exit_status = wait_for_exit(process);

            assert!(exit_status.success());
            assert_eq!(tracker.kills(), 1);
        }

        #[test]
        fn shutdown_kills_the_group_once() {
            let tracker = CountingProcessGroupTracker::default();
            let process = spawn_in_group(&ExecutableData::long_running(), &tracker);

            process.shutdown().unwrap();

            assert_eq!(tracker.kills(), 1);
        }

        #[test]
        fn force_kill_kills_the_group_once() {
            let tracker = CountingProcessGroupTracker::default();
            let process = spawn_in_group(&ExecutableData::long_running(), &tracker);

            let exit_status = process.force_kill().unwrap();

            assert!(!exit_status.success());
            assert_eq!(tracker.kills(), 1);
        }

        #[test]
        fn running_process_does_not_kill_the_group() {
            let tracker = CountingProcessGroupTracker::default();
            let process = spawn_in_group(&ExecutableData::long_running(), &tracker);

            let ProcessState::Running(process) = process.try_wait() else {
                panic!("expected process to be running");
            };

            assert_eq!(tracker.kills(), 0);
            process.shutdown().unwrap();
        }

        #[test]
        fn a_failing_group_kill_does_not_change_the_exit_status() {
            let tracker = CountingProcessGroupTracker::with_failing_kill();
            let process = spawn_in_group(&ExecutableData::short_lived(), &tracker);

            let exit_status = wait_for_exit(process);

            assert!(exit_status.success());
            assert_eq!(tracker.kills(), 1);
        }
    }
}
