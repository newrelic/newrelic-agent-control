//! Windows Job Object wrapper for terminating a process and its descendants as a group.
//!
//! PoC: as of the on-host crash-survival CDD, a job created here is *not* configured to
//! auto-kill its members when its last handle closes — see [`JobObject::new`].

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use tracing::error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};

/// Error produced by a Windows Job Object operation.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct JobObjectError(String);

/// Represents a Windows Job Object used to manage and control a group of processes.
/// When the Job Object is explicitly killed (or dropped during Agent Control's own orderly
/// shutdown/restart-policy teardown), all associated processes are terminated.
pub struct JobObject {
    handle: HANDLE,
}
impl JobObject {
    /// Creates a new JobObject.
    ///
    /// PoC: deliberately does *not* set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. That flag would
    /// have the kernel terminate every process in the job the instant this job's last handle
    /// closes — which happens automatically, with no code of ours involved, when Agent
    /// Control's own process dies abruptly (a crash never runs `Drop`, so our own explicit
    /// `kill()`/`TerminateJobObject` calls never get a chance to run either way; it's the
    /// kernel's automatic handle-close cleanup specifically that this avoids). Without it, a
    /// sub-agent assigned to this job survives an Agent Control crash as an orphan, the
    /// Windows analog of the on-host crash-survival CDD/PoC's `KillMode=process` on Linux.
    /// Agent Control's own deliberate teardown (`JobObject::kill`, called from
    /// `CommandOSStarted`'s shutdown path, and the `Drop` impl below) is unaffected: both call
    /// `TerminateJobObject` directly and don't rely on this flag at all.
    pub fn new() -> Result<Self, JobObjectError> {
        unsafe {
            let handle = CreateJobObjectW(None, None)
                .map_err(|e| JobObjectError(format!("creating JobObject: {e}")))?;

            Ok(Self { handle })
        }
    }

    /// Assigns the given process to this JobObject. The process will be terminated when the JobObject is closed.
    pub fn assign_process(&self, process: &Child) -> Result<(), JobObjectError> {
        unsafe {
            let process_handle = HANDLE(process.as_raw_handle());
            AssignProcessToJobObject(self.handle, process_handle)
                .map_err(|e| JobObjectError(format!("assigning process to JobObject: {e}")))?;
        }
        Ok(())
    }

    /// Kills the JobObject, terminating all associated processes.
    pub fn kill(self) -> Result<(), JobObjectError> {
        unsafe {
            TerminateJobObject(self.handle, 0)
                .map_err(|e| JobObjectError(format!("closing JobObject handle: {e}")))?;
        }
        Ok(())
    }
}

/// Ensure the JobObject is killed when dropped.
impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            let _ = TerminateJobObject(self.handle, 0)
                .inspect_err(|err| error!(%err,"Fail to kill a JobObject"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::retry::retry;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn test_job_object_kills_process() {
        let job = JobObject::new().expect("Failed to create JobObject");
        let mut child = Command::new("cmd")
            .args(["/C", "timeout", "/T", "15"])
            .spawn()
            .expect("Failed to spawn process");

        job.assign_process(&child)
            .expect("Failed to assign process to JobObject");

        job.kill().unwrap();
        retry(100, Duration::from_millis(100), || {
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                Ok::<(), &str>(())
            } else {
                Err("process still running")
            }
        })
        .expect("Failed to wait on process");
    }

    /// PoC: proves the actual crash-survival property, not just that `kill()`/`Drop` still
    /// work. A real Agent Control crash never runs `Drop` (no unwinding on an abrupt
    /// process death) and never calls `kill()` either — the kernel just closes every handle
    /// Agent Control held, automatically, including this job's. `mem::forget` skips `Drop`
    /// the same way a crash would, then closing the raw handle directly (bypassing our own
    /// `kill()`/`Drop` code entirely) reproduces exactly what the kernel does on process
    /// death. If `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` were still set, that handle close alone
    /// would terminate the child; without it, as asserted below, the child keeps running.
    #[test]
    fn job_handle_closing_without_an_explicit_kill_does_not_kill_the_child() {
        let job = JobObject::new().expect("Failed to create JobObject");
        let mut child = Command::new("cmd")
            .args(["/C", "timeout", "/T", "15"])
            .spawn()
            .expect("Failed to spawn process");

        job.assign_process(&child)
            .expect("Failed to assign process to JobObject");

        let handle = job.handle;
        std::mem::forget(job);
        unsafe {
            windows::Win32::Foundation::CloseHandle(handle)
                .expect("Failed to close the job's raw handle");
        }

        std::thread::sleep(Duration::from_millis(500));
        assert!(
            child.try_wait().unwrap().is_none(),
            "child should still be running after the job's last handle closed with no explicit kill"
        );

        child.kill().expect("Failed to clean up the child");
        child.wait().expect("Failed to wait on the child");
    }
}
