use super::spawned_process::error::CommandError;
use super::spawned_process::os_process::OsProcess;
use super::{ProcessState, SupervisedProcess};
use crate::sub_agent::on_host::process_group_tracker::{TrackedProcessGroup, kill_group};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use tracing::warn;

impl<G: TrackedProcessGroup> SupervisedProcess<G> {
    /// Supervises a process just spawned by this run. The `Child` handle is dropped on return, the
    /// process keeps running and is managed by its pid alone.
    pub(super) fn from_started(os_process: OsProcess<G>) -> Result<Self, CommandError> {
        Ok(Self {
            pid: os_process.get_pid(),
            agent_id: os_process.agent_id().clone(),
            shutdown_timeout: os_process.shutdown_timeout(),
            group: os_process.into_group(),
        })
    }

    /// Non-blocking check of whether the process has exited. Uses `WNOHANG` so `waitpid` returns
    /// immediately; any status that isn't a clean exit or signal termination is treated as still
    /// running, including ptrace stops and `ECHILD` (adopted/re-parented processes).
    pub fn try_wait(self) -> ProcessState<G> {
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
                // The process is reaped, so nothing it left behind is a zombie of the group.
                kill_group(self.group, &self.agent_id);
                ProcessState::Exited(Self::raw_exit_status(status))
            }
            // Not our child: its exit cannot be observed.
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

    /// Sends SIGTERM to the entire process group. ESRCH (already gone) is treated as success.
    ///
    /// The sub-agent is spawned as its own process group leader (pgid = pid), so negating the
    /// pid targets every process it has spawned, not just the root process.
    pub(super) fn graceful_signal(&self) -> Result<(), CommandError> {
        match kill(Pid::from_raw(-(self.pid as i32)), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(e) => Err(CommandError::IOError(io::Error::from(e))),
        }
    }

    /// Sends SIGKILL and blocks until the process exits, then kills what is left in its group.
    /// The group goes after reaping the process, since a zombie still counts as a member.
    pub(super) fn force_kill(self) -> Result<ExitStatus, CommandError> {
        let pid = Pid::from_raw(self.pid as i32);
        match kill(pid, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {
                let exit_status = self.wait();
                kill_group(self.group, &self.agent_id);
                exit_status
            }
            Err(e) => {
                kill_group(self.group, &self.agent_id);
                Err(CommandError::IOError(io::Error::from(e)))
            }
        }
    }

    /// Blocks until the process exits. `EINTR` (signal interrupted the syscall) restarts the wait
    /// rather than surfacing an error, matching the POSIX idiom for restartable waits.
    pub(super) fn wait(&self) -> Result<ExitStatus, CommandError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;
    use crate::sub_agent::on_host::process_group_tracker::PassthroughProcessGroup;
    use nix::unistd::getpgid;

    fn spawn(bin: &str, args: &[&str]) -> SupervisedProcess<PassthroughProcessGroup> {
        SupervisedProcess::spawn(OsProcessConfig::new(
            "test".to_string().try_into().unwrap(),
            &ExecutableData {
                id: "test".to_string(),
                bin: bin.to_string(),
                args: args.iter().map(|a| a.to_string()).collect(),
                env: Default::default(),
                restart_policy: Default::default(),
                shutdown_timeout: Default::default(),
            },
            None,
            PassthroughProcessGroup,
        ))
        .unwrap()
    }

    #[test]
    fn spawned_process_leads_its_own_process_group() {
        let process = spawn("sleep", &["5"]);
        let pid = Pid::from_raw(process.pid as i32);

        // `graceful_signal` targets `-pid`, which is only the child's group (and not Agent
        // Control's own) if the child is a group leader distinct from this process.
        assert_eq!(getpgid(Some(pid)).unwrap(), pid);
        assert_ne!(getpgid(Some(pid)).unwrap(), getpgid(None).unwrap());

        process.force_kill().unwrap();
    }

    #[test]
    fn graceful_signal_succeeds_when_process_group_is_gone() {
        let process = spawn("true", &[]);
        // Reap the child so its process group no longer exists.
        waitpid(Pid::from_raw(process.pid as i32), None).unwrap();

        assert!(process.graceful_signal().is_ok());
    }
}
