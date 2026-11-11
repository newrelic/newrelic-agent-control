use super::spawned_process::error::CommandError;
use super::{ProcessState, SupervisedProcess};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use tracing::warn;

impl SupervisedProcess {
    /// Non-blocking check of whether the process has exited. Uses `WNOHANG` so `waitpid` returns
    /// immediately; any status that isn't a clean exit or signal termination is treated as still
    /// running, including ptrace stops and `ECHILD` (adopted/re-parented processes).
    pub fn try_wait(self) -> ProcessState {
        let pid = Pid::from_raw(self.pid as i32);
        // waitpid(2) with WNOHANG can return ECHILD, EINTR, or EINVAL; see
        // <https://linux.die.net/man/2/waitpid>
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive)
            | Ok(WaitStatus::Stopped(..))
            | Ok(WaitStatus::Continued(..)) => ProcessState::Running(self),
            #[cfg(target_os = "linux")]
            Ok(WaitStatus::PtraceEvent(..) | WaitStatus::PtraceSyscall(..)) => {
                ProcessState::Running(self)
            }
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => {
                ProcessState::Exited(Self::raw_exit_status(status))
            }
            // Not our child: adopted process. Full liveness check will be wired up
            // when ProcessOrigin::Adopt is implemented.
            Err(nix::errno::Errno::ECHILD) => ProcessState::Running(self),
            // Remaining errors evaluate to considering the process to be still running,
            // a safer outcome than risking having orphan processes around treated as
            // stopped, incurring in re-spawn, double-handling, resource leaks and other
            // problems that are hard to troubleshoot.
            Err(nix::errno::Errno::EINTR) => {
                warn!(pid = self.pid, agent_id = %self.agent_id, "EINTR: WNOHANG is not set when it should and an unblocked signal or a SIGCHLD was caught");
                ProcessState::Running(self)
            }
            Err(nix::errno::Errno::EINVAL) => {
                warn!(pid = self.pid, agent_id = %self.agent_id, "EINVAL: The options argument was invalid");
                ProcessState::Running(self)
            }
            // other `Errno`s that are not expected to occur according to `waitpid`'s spec
            Err(e) => {
                warn!(pid = self.pid, agent_id = %self.agent_id, "Unexpected error on call to waitpid: {e}; treating process as running");
                ProcessState::Running(self)
            }
        }
    }

    /// Sends SIGTERM. ESRCH (already gone) is treated as success since the goal —
    /// process is not running — is already achieved.
    pub(super) fn graceful_signal(&self) -> Result<(), CommandError> {
        match kill(Pid::from_raw(self.pid as i32), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(e) => Err(CommandError::IOError(io::Error::from(e))),
        }
    }

    /// Sends SIGKILL and blocks until the process exits.
    pub(super) fn force_kill(self) -> Result<ExitStatus, CommandError> {
        let pid = Pid::from_raw(self.pid as i32);
        match kill(pid, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => self.wait(),
            Err(e) => Err(CommandError::IOError(io::Error::from(e))),
        }
    }

    /// Blocks until the process exits. `EINTR` (signal interrupted the syscall) restarts the wait
    /// rather than surfacing an error, matching the POSIX idiom for restartable waits.
    pub(super) fn wait(self) -> Result<ExitStatus, CommandError> {
        let pid = Pid::from_raw(self.pid as i32);
        loop {
            match waitpid(pid, None) {
                Ok(status) => break Ok(Self::raw_exit_status(status)),
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => break Err(CommandError::IOError(io::Error::from(e))),
            }
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
