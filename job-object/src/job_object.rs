//! Windows Job Object wrapper for terminating a process and its descendants as a group.

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use tracing::error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// Error produced by a Windows Job Object operation.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct JobObjectError(String);

/// What happens to the processes of a [`JobObject`] when it is closed or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClose {
    /// The processes are terminated.
    Kill,
    /// The processes are left running.
    Detach,
}

/// Represents a Windows Job Object used to manage and control a group of processes.
/// When the Job Object is killed, all associated processes are terminated. When it is dropped,
/// they are terminated or left running according to its [`OnClose`].
pub struct JobObject {
    handle: HANDLE,
    on_close: OnClose,
}

impl JobObject {
    /// Creates a new JobObject whose processes are terminated or left running, according to
    /// `on_close`, when it is closed or dropped.
    pub fn new(on_close: OnClose) -> Result<Self, JobObjectError> {
        unsafe {
            let handle = CreateJobObjectW(None, None)
                .map_err(|e| JobObjectError(format!("creating JobObject: {e}")))?;

            if on_close == OnClose::Kill {
                // Set the JobObject to kill all associated processes when the JobObject is closed.
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .map_err(|e| JobObjectError(format!("setting JobObject information: {e}")))?;
            }

            Ok(Self { handle, on_close })
        }
    }

    /// Assigns the given process to this JobObject. The process will be terminated when the JobObject is closed, unless it was created with [`OnClose::Detach`].
    pub fn assign_process(&self, process: &Child) -> Result<(), JobObjectError> {
        unsafe {
            let process_handle = HANDLE(process.as_raw_handle());
            AssignProcessToJobObject(self.handle, process_handle)
        }
        .map_err(|e| JobObjectError(format!("assigning process to JobObject: {e}")))
    }

    /// Terminates all processes in the JobObject, consuming it to prevent double-kills.
    pub fn kill(self) -> Result<(), JobObjectError> {
        unsafe { TerminateJobObject(self.handle, 0) }
            .map_err(|e| JobObjectError(format!("terminating JobObject: {e}")))
    }
}

/// Ensure the JobObject is killed when dropped, unless it was created with [`OnClose::Detach`].
impl Drop for JobObject {
    fn drop(&mut self) {
        if self.on_close == OnClose::Detach {
            return;
        }
        unsafe {
            let _ = TerminateJobObject(self.handle, 0)
                .inspect_err(|err| error!(%err,"Fail to kill a JobObject"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn test_job_object_detaching_on_close_leaves_process_running_when_dropped() {
        let job = JobObject::new(OnClose::Detach).expect("Failed to create JobObject");
        // `ping` runs for ~15s without a console, unlike `timeout` which exits right away without one.
        let mut child = Command::new("ping")
            .args(["-n", "16", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("Failed to spawn process");
        job.assign_process(&child)
            .expect("Failed to assign process to JobObject");

        drop(job);
        std::thread::sleep(Duration::from_millis(500));

        let status = child.try_wait().unwrap();
        child.kill().unwrap();
        assert!(status.is_none(), "process exited with {status:?}");
    }

    #[test]
    fn test_job_object_kills_process() {
        let job = JobObject::new(OnClose::Kill).expect("Failed to create JobObject");
        let mut child = Command::new("cmd")
            .args(["/C", "timeout", "/T", "15"])
            .spawn()
            .expect("Failed to spawn process");

        job.assign_process(&child)
            .expect("Failed to assign process to JobObject");

        job.kill().unwrap();
        let exited = (0..100).any(|_| {
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
            false
        });
        assert!(exited, "Failed to wait on process");
    }
}
