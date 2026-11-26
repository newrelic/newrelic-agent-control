//! Process tracking on unix targets.

use super::{ProcessGroupTracker, ProcessGroupTrackerError, TrackedProcessGroup};
use crate::agent_control::agent_id::AgentID;
use std::process::{Child, Command};

/// Spawns the processes of every executable as they are, without keeping them in any group.
#[derive(Debug, Clone)]
pub struct PassthroughProcessGroupTracker;

impl ProcessGroupTracker for PassthroughProcessGroupTracker {
    type Group = PassthroughProcessGroup;

    fn group(
        &self,
        _agent_id: &AgentID,
        _exec_id: &str,
    ) -> Result<PassthroughProcessGroup, ProcessGroupTrackerError> {
        Ok(PassthroughProcessGroup)
    }
}

/// Group of the passthrough tracker: it holds no processes, so there is nothing to kill.
#[derive(Debug)]
pub struct PassthroughProcessGroup;

impl TrackedProcessGroup for PassthroughProcessGroup {
    fn spawn(&self, mut command: Command) -> Result<Child, ProcessGroupTrackerError> {
        command
            .spawn()
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }

    fn kill(self) -> Result<(), ProcessGroupTrackerError> {
        Ok(())
    }
}
