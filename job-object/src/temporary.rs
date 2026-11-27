//! Unnamed Job Objects, which terminate their processes when this process stops using them.

use crate::job_handle::{JobHandle, JobObjectError};
use std::ops::Deref;
use tracing::error;
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
};
use windows::core::Owned;

/// Unnamed Job Object with the "kill on job close" configuration: its processes are terminated
/// when it is killed or dropped, and by the kernel if this process exits without dropping it.
pub struct TemporaryJobObject(JobHandle);

impl TemporaryJobObject {
    /// Creates a new unnamed JobObject with the "kill on job close" configuration.
    pub fn new() -> Result<Self, JobObjectError> {
        let handle = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| JobObjectError(format!("creating JobObject: {e}")))?;
        // Safety: the handle was just created and is owned here.
        let job = Self(JobHandle {
            handle: unsafe { Owned::new(handle) },
        });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        job.set_extended_limit_information(&limits)?;
        Ok(job)
    }

    /// Terminates all processes in the JobObject, consuming it so it cannot be used afterwards.
    /// Dropping it then terminates the processes again, which is harmless since none are left.
    pub fn kill(self) -> Result<(), JobObjectError> {
        self.terminate()
    }
}

impl Deref for TemporaryJobObject {
    type Target = JobHandle;

    fn deref(&self) -> &JobHandle {
        &self.0
    }
}

/// Ensure the JobObject is killed when dropped.
impl Drop for TemporaryJobObject {
    fn drop(&mut self) {
        let _ = self
            .terminate()
            .inspect_err(|err| error!(%err, "Fail to kill a JobObject"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_handle::tests::{EXIT_TIMEOUT, exits_within, long_running_process};

    #[test]
    fn test_kill_terminates_the_processes() {
        let job = TemporaryJobObject::new().unwrap();
        let mut child = long_running_process();
        job.assign_process(&child).unwrap();

        job.kill().unwrap();

        assert!(
            exits_within(&mut child, EXIT_TIMEOUT),
            "process kept running"
        );
    }

    #[test]
    fn test_drop_terminates_the_processes() {
        let job = TemporaryJobObject::new().unwrap();
        let mut child = long_running_process();
        job.assign_process(&child).unwrap();

        drop(job);

        assert!(
            exits_within(&mut child, EXIT_TIMEOUT),
            "process kept running"
        );
    }
}
