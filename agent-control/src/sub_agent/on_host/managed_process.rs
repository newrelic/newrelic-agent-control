//! A process supervised by Agent Control, either spawned by it or adopted from a previous
//! instance, exposing the operations the supervisor needs regardless of its origin.

pub mod spawned_process;

use crate::agent_control::agent_id::AgentID;
use spawned_process::{error::CommandError, os_process::OsProcessConfig};
use std::process::ExitStatus;
use std::time::Duration;
#[cfg(target_family = "unix")]
use std::time::Instant;
#[cfg(target_family = "unix")]
use tracing::warn;

#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

// ---- Windows process handle wrapper ------------------------------------------------------------

/// Owns a Windows process HANDLE and closes it on drop, keeping the process object alive long
/// enough to reliably retrieve the exit code even for very short-lived processes.
#[cfg(target_family = "windows")]
struct ProcessHandle(windows::Win32::Foundation::HANDLE);

#[cfg(target_family = "windows")]
impl ProcessHandle {
    /// Opens a secondary handle to `os_process`, guaranteeing it refers to the same process
    /// object. Safe because `os_process` holds the `Child` handle that keeps the kernel object
    /// alive, so the PID cannot be recycled before this call returns.
    fn from_os_process(
        os_process: &spawned_process::os_process::OsProcess,
    ) -> Result<Self, CommandError> {
        Self::open(os_process.get_pid())
    }

    /// Opens a handle to an already-running process by PID alone. The caller must ensure the
    /// PID has not been recycled since it was observed (no `Child` handle to pin it).
    fn open(pid: u32) -> Result<Self, CommandError> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map(Self)
                .map_err(std::io::Error::from)
                .map_err(CommandError::from)
        }
    }

    /// Returns the current exit code, or `None` if the handle is no longer accessible.
    /// `Some(259)` means the process is still running (STILL_ACTIVE).
    fn exit_code(&self) -> Option<u32> {
        use windows::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.0, &mut code).ok().map(|_| code) }
    }
}

#[cfg(target_family = "windows")]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

// ---- Acquisition -------------------------------------------------------------------------------

/// How Agent Control acquires a process to supervise.
///
/// Once [`Acquisition::acquire`] is called, both paths yield an identical [`SupervisedProcess`]
/// regardless of origin.
// Acquisition is consumed immediately by acquire(); it is never stored, so the stack size
// difference between Spawn (which carries OsProcessConfig) and Adopt is not a concern.
#[allow(clippy::large_enum_variant)]
pub enum Acquisition {
    /// Start a new process from the given config, then immediately detach from it.
    Spawn(OsProcessConfig),
    /// Supervise a process that was already running when this instance started.
    ///
    /// Not yet active: constructed only once per-instance bookkeeping (PID files, cgroup
    /// inspection, etc.) is in place. Until then this variant is intentionally unreachable.
    #[allow(dead_code)]
    Adopt {
        pid: u32,
        agent_id: AgentID,
        shutdown_timeout: Duration,
    },
}

impl Acquisition {
    /// Executes the acquisition and returns a [`SupervisedProcess`] ready to manage.
    pub fn acquire(self) -> Result<SupervisedProcess, CommandError> {
        match self {
            Self::Spawn(config) => {
                let os_process = config.start()?.stream()?;
                let pid = os_process.get_pid();
                let agent_id = os_process.agent_id().clone();
                let shutdown_timeout = os_process.shutdown_timeout();
                #[cfg(target_family = "windows")]
                let job_object = Some(os_process.create_job_object()?);
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
            Self::Adopt {
                pid,
                agent_id,
                shutdown_timeout,
            } => {
                // Job-object wiring is deferred to when Acquisition::Adopt is fully
                // implemented; remove the Option from the field when that happens.
                #[cfg(target_family = "windows")]
                let job_object: Option<JobObject> = None;
                #[cfg(target_family = "windows")]
                let process_handle = ProcessHandle::open(pid)?;
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

// ---- Process state types -----------------------------------------------------------------------

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
    /// `None` only for adopted processes, whose job-object wiring is not yet implemented;
    /// remove the `Option` when `Acquisition::Adopt` is fully implemented.
    #[cfg(target_family = "windows")]
    job_object: Option<JobObject>,
    /// Keeps the process object alive so the exit code remains retrievable even after the
    /// process exits quickly and before we get a chance to query it.
    #[cfg(target_family = "windows")]
    process_handle: ProcessHandle,
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

// ---- SupervisedProcess public API --------------------------------------------------------------

impl SupervisedProcess {
    /// Returns the process id.
    pub fn get_pid(&self) -> u32 {
        self.pid
    }

    /// Non-blocking liveness check. Returns [`ProcessState::Running`] with ownership of `self`
    /// if the process is still alive, or [`ProcessState::Exited`] once it has exited.
    pub fn try_wait(self) -> ProcessState {
        self.platform_try_wait()
    }

    /// Stops the process, waits for it to exit, and returns the result.
    pub fn shutdown(self) -> Result<ExitedProcess, CommandError> {
        self.platform_shutdown()
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

// ---- Unix implementation -----------------------------------------------------------------------

#[cfg(target_family = "unix")]
impl SupervisedProcess {
    fn platform_try_wait(self) -> ProcessState {
        use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
        use nix::unistd::Pid;

        let pid = Pid::from_raw(self.pid as i32);
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => ProcessState::Running(self),
            Ok(status) => ProcessState::Exited(self.into_exited(Self::raw_exit_status(status))),
            Err(nix::errno::Errno::ECHILD) => {
                // Not our child: adopted process. Full liveness check will be wired up
                // when Acquisition::Adopt is implemented.
                ProcessState::Running(self)
            }
            Err(_) => ProcessState::Running(self),
        }
    }

    fn wait(self) -> Result<ExitedProcess, CommandError> {
        use nix::sys::wait::waitpid;
        use nix::unistd::Pid;

        let pid = Pid::from_raw(self.pid as i32);
        loop {
            match waitpid(pid, None) {
                Ok(status) => break Ok(self.into_exited(Self::raw_exit_status(status))),
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => break Err(CommandError::IOError(std::io::Error::from(e))),
            }
        }
    }

    fn platform_shutdown(self) -> Result<ExitedProcess, CommandError> {
        let pid_raw = self.pid as i32;
        let agent_id_display = self.agent_id.clone();
        let shutdown_timeout = self.shutdown_timeout;

        if let Err(e) = Self::sigterm(pid_raw) {
            warn!(agent_id = %agent_id_display, pid = pid_raw, %e, "Graceful shutdown failed. Stopping forcefully");
            self.kill()
        } else {
            let deadline = Instant::now() + shutdown_timeout;
            let mut current = self;
            loop {
                match current.try_wait() {
                    ProcessState::Exited(exited) => break Ok(exited),
                    ProcessState::Running(running) => {
                        if Instant::now() >= deadline {
                            warn!(agent_id = %agent_id_display, pid = pid_raw, "Graceful shutdown timeout exceeded. Stopping forcefully");
                            break running.kill();
                        }
                        std::thread::sleep(POLL_INTERVAL);
                        current = running;
                    }
                }
            }
        }
    }

    /// Sends SIGKILL and waits for the process to exit.
    fn kill(self) -> Result<ExitedProcess, CommandError> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let pid = Pid::from_raw(self.pid as i32);
        match kill(pid, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => self.wait(),
            Err(e) => Err(CommandError::IOError(std::io::Error::from(e))),
        }
    }

    /// Sends SIGTERM to `pid_raw`. ESRCH (already gone) is treated as success since the goal —
    /// process is not running — is already achieved.
    fn sigterm(pid_raw: i32) -> Result<(), CommandError> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        match kill(Pid::from_raw(pid_raw), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(e) => Err(CommandError::IOError(std::io::Error::from(e))),
        }
    }

    /// Converts a nix `WaitStatus` into a `std::process::ExitStatus` by reconstructing the raw
    /// waitpid status word that `ExitStatus::from_raw` expects.
    fn raw_exit_status(status: nix::sys::wait::WaitStatus) -> ExitStatus {
        use nix::sys::wait::WaitStatus;
        use std::os::unix::process::ExitStatusExt;

        let raw = match status {
            // Normal exit: exit code occupies bits 8–15.
            WaitStatus::Exited(_, code) => (code & 0xFF) << 8,
            // Signal termination: signal number in bits 0–6, core-dump flag in bit 7.
            WaitStatus::Signaled(_, signal, core_dumped) => {
                (signal as i32 & 0x7F) | (if core_dumped { 0x80 } else { 0 })
            }
            _ => 0,
        };
        ExitStatus::from_raw(raw)
    }
}

// ---- Windows implementation --------------------------------------------------------------------

#[cfg(target_family = "windows")]
impl SupervisedProcess {
    fn platform_try_wait(self) -> ProcessState {
        use std::os::windows::process::ExitStatusExt;
        match self.process_handle.exit_code() {
            Some(259) => ProcessState::Running(self),
            Some(code) => ProcessState::Exited(self.into_exited(ExitStatus::from_raw(code))),
            None => ProcessState::Exited(self.into_exited(ExitStatus::from_raw(1))),
        }
    }

    fn platform_shutdown(self) -> Result<ExitedProcess, CommandError> {
        use std::os::windows::process::ExitStatusExt;
        // Destructure to consume job_object independently of the other fields.
        let SupervisedProcess {
            pid,
            agent_id,
            shutdown_timeout: _,
            job_object,
            process_handle,
        } = self;
        // TerminateJobObject terminates the process and all processes it has spawned.
        if let Some(job) = job_object {
            job.kill().map_err(CommandError::from)?;
        }
        // Spin-poll using the retained handle until the process reports a final exit code.
        loop {
            match process_handle.exit_code() {
                Some(259) => std::thread::sleep(POLL_INTERVAL),
                Some(code) => {
                    return Ok(ExitedProcess {
                        pid,
                        agent_id,
                        exit_status: ExitStatus::from_raw(code),
                    });
                }
                None => {
                    return Ok(ExitedProcess {
                        pid,
                        agent_id,
                        exit_status: ExitStatus::from_raw(1),
                    });
                }
            }
        }
    }
}

// ---- Tests -------------------------------------------------------------------------------------

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
            let process = Acquisition::Spawn(OsProcessConfig::new(
                AgentID::AgentControl,
                self,
                SubAgentFileLoggingConfig::default(),
            ))
            .acquire()
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
