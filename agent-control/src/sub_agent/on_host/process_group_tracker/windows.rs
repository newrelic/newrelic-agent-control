//! Process tracking with Job Objects.

use super::{ProcessGroupTracker, ProcessGroupTrackerError, TrackedProcessGroup};
use crate::agent_control::agent_id::AgentID;
use job_object::{JobObject, OnClose};
use std::process::{Child, Command};

/// Keeps the processes of every executable in a Job Object of its own.
#[derive(Debug, Clone)]
pub struct JobObjectProcessGroupTracker {
    on_close: OnClose,
}

impl JobObjectProcessGroupTracker {
    /// Creates a tracker whose Job Objects terminate or leave running their processes, according
    /// to `on_close`, when they are closed.
    pub fn new(on_close: OnClose) -> Self {
        Self { on_close }
    }
}

impl ProcessGroupTracker for JobObjectProcessGroupTracker {
    type Group = JobObjectProcessGroup;

    fn group(
        &self,
        _agent_id: &AgentID,
        _exec_id: &str,
    ) -> Result<JobObjectProcessGroup, ProcessGroupTrackerError> {
        JobObject::new(self.on_close)
            .map(JobObjectProcessGroup)
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }
}

/// The Job Object of an executable. Its processes are killed when the group is dropped, unless it
/// was created with [`OnClose::Detach`].
pub struct JobObjectProcessGroup(JobObject);

impl TrackedProcessGroup for JobObjectProcessGroup {
    fn spawn(&self, mut command: Command) -> Result<Child, ProcessGroupTrackerError> {
        let mut child = command
            .spawn()
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))?;
        if let Err(err) = self.0.assign_process(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessGroupTrackerError(err.to_string()));
        }
        Ok(child)
    }

    fn kill(self) -> Result<(), ProcessGroupTrackerError> {
        self.0
            .kill()
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }
}
