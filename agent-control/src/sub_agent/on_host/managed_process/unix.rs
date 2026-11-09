use super::spawned_process::error::CommandError;
use super::{POLL_INTERVAL, ProcessState, SupervisedProcess};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Instant;
use tracing::warn;

impl SupervisedProcess {
    pub fn try_wait(self) -> ProcessState {
        let pid = Pid::from_raw(self.pid as i32);
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive)
            | Ok(WaitStatus::Stopped(..))
            | Ok(WaitStatus::Continued(..)) => ProcessState::Running(self),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            Ok(WaitStatus::PtraceEvent(..) | WaitStatus::PtraceSyscall(..)) => {
                ProcessState::Running(self)
            }
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => {
                ProcessState::Exited(self.into_exit_status(Self::raw_exit_status(status)))
            }
            Err(nix::errno::Errno::ECHILD) => {
                // Not our child: adopted process. Full liveness check will be wired up
                // when ProcessOrigin::Adopt is implemented.
                ProcessState::Running(self)
            }
            Err(_) => ProcessState::Running(self),
        }
    }

    pub(super) fn wait(self) -> Result<ExitStatus, CommandError> {
        let pid = Pid::from_raw(self.pid as i32);
        loop {
            match waitpid(pid, None) {
                Ok(status) => break Ok(self.into_exit_status(Self::raw_exit_status(status))),
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => break Err(CommandError::IOError(std::io::Error::from(e))),
            }
        }
    }

    pub fn shutdown(self) -> Result<ExitStatus, CommandError> {
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
                    ProcessState::Exited(exit_status) => break Ok(exit_status),
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
    fn kill(self) -> Result<ExitStatus, CommandError> {
        let pid = Pid::from_raw(self.pid as i32);
        match kill(pid, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => self.wait(),
            Err(e) => Err(CommandError::IOError(std::io::Error::from(e))),
        }
    }

    /// Sends SIGTERM to `pid_raw`. ESRCH (already gone) is treated as success since the goal —
    /// process is not running — is already achieved.
    fn sigterm(pid_raw: i32) -> Result<(), CommandError> {
        match kill(Pid::from_raw(pid_raw), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(e) => Err(CommandError::IOError(std::io::Error::from(e))),
        }
    }

    /// Converts a nix `WaitStatus` into a `std::process::ExitStatus` by reconstructing the raw
    /// waitpid status word that `ExitStatus::from_raw` expects.
    fn raw_exit_status(status: nix::sys::wait::WaitStatus) -> ExitStatus {
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
