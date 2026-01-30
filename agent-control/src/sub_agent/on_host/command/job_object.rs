use crate::sub_agent::on_host::command::error::CommandError;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use tracing::error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// Represents a Windows Job Object used to manage and control a group of processes.
/// When the Job Object is killed or dropped, all associated processes are terminated.
pub struct JobObject {
    handle: HANDLE,
}
impl JobObject {
    /// Creates a new JobObject with the "kill on job close" configuration.
    pub fn new() -> Result<Self, CommandError> {
        unsafe {
            let handle = CreateJobObjectW(None, None)
                .map_err(|e| CommandError::WinError(format!("creating JobObject: {e}")))?;

            // Set the JobObject to kill all associated processes when the JobObject is closed.
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| CommandError::WinError(format!("setting JobObject information: {e}")))?;

            Ok(Self { handle })
        }
    }

    /// Assigns the given process to this JobObject. The process will be terminated when the JobObject is closed.
    pub fn assign_process(&self, process: &Child) -> Result<(), CommandError> {
        unsafe {
            let process_handle = HANDLE(process.as_raw_handle());
            AssignProcessToJobObject(self.handle, process_handle).map_err(|e| {
                CommandError::WinError(format!("assigning process to JobObject: {e}"))
            })?;
        }
        Ok(())
    }

    /// Kills the JobObject, terminating all associated processes.
    pub fn kill(self) -> Result<(), CommandError> {
        unsafe {
            TerminateJobObject(self.handle, 0)
                .map_err(|e| CommandError::WinError(format!("closing JobObject handle: {e}")))?;
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
}
