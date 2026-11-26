//! Tracking of the processes of an executable, so everything it spawns can be stopped together.

use crate::agent_control::agent_id::AgentID;
use std::fmt::Debug;
use std::process::{Child, Command};

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "unix")]
pub use unix::{PassthroughProcessGroup, PassthroughProcessGroupTracker};

#[cfg(target_family = "windows")]
mod windows;
#[cfg(target_family = "windows")]
pub use windows::{JobObjectProcessGroup, JobObjectProcessGroupTracker};

#[cfg(target_family = "unix")]
pub type PlatformProcessGroupTracker = PassthroughProcessGroupTracker;
#[cfg(target_family = "windows")]
pub type PlatformProcessGroupTracker = JobObjectProcessGroupTracker;

/// Error of an operation of a process tracker, whatever mechanism it uses.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ProcessGroupTrackerError(String);

/// Keeps the processes of every executable in a group the operating system can kill together.
///
/// A group is the operating system object (e.g. a Job Object) that holds the processes of one
/// executable of an agent. Agent Control keeps it to stop the whole process tree, and trackers
/// that give groups a stable identity can also use it to find the processes of a previous
/// Agent Control run and adopt them.
pub trait ProcessGroupTracker: Clone + Send + 'static {
    type Group: TrackedProcessGroup;

    /// Group of the processes of one executable of an agent. `agent_id` and `exec_id` identify
    /// the executable, so trackers with named groups get the same group on every run.
    fn group(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
    ) -> Result<Self::Group, ProcessGroupTrackerError>;

    // Planned (adoption): the process of a previous Agent Control run to keep supervising for the
    // executable, `None` if it has to be spawned. Job Objects are anonymous, so their tracker
    // always returns `None`.
    //
    // fn adopt(
    //     &self,
    //     agent_id: &AgentID,
    //     exec_data: &ExecutableData,
    // ) -> Option<AdoptConfig<Self::Group>>;
}

/// The processes of one executable, kept together by the operating system.
pub trait TrackedProcessGroup {
    /// Starts the command as a member of the group.
    fn spawn(&self, command: Command) -> Result<Child, ProcessGroupTrackerError>;

    /// Kills every member and removes the group.
    fn kill(self) -> Result<(), ProcessGroupTrackerError>;

    // Planned (adoption), used to inspect and stop the group of a previous run. Trackers that
    // cannot answer return an explicit "not supported" error.
    //
    // /// Process the executable was started as, `None` if it is gone.
    // fn leader(&self) -> Result<Option<u32>, ProcessGroupTrackerError>;
    //
    // fn has_members(&self) -> Result<bool, ProcessGroupTrackerError>;
    //
    // /// Asks every member to stop, without waiting for them.
    // fn terminate(&self) -> Result<(), ProcessGroupTrackerError>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
    use crate::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;
    use std::path::PathBuf;

    /// Tracker the tests that do not exercise tracking run with.
    pub type TestProcessGroupTracker = PlatformProcessGroupTracker;

    pub type TestProcessGroup = <TestProcessGroupTracker as ProcessGroupTracker>::Group;

    /// Process config that runs on every target.
    pub fn test_process_config(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        maybe_log_dir: Option<PathBuf>,
    ) -> OsProcessConfig<TestProcessGroup> {
        let group = TestProcessGroupTracker::default()
            .group(&agent_id, &executable_data.id)
            .expect("a test group can always be created");
        OsProcessConfig::new(agent_id, executable_data, maybe_log_dir, group)
    }

    /// Tracker that cannot create the group of an executable, as with an invalid executable id.
    #[derive(Clone)]
    pub struct FailingProcessGroupTracker;

    impl ProcessGroupTracker for FailingProcessGroupTracker {
        type Group = TestProcessGroup;

        fn group(
            &self,
            _agent_id: &AgentID,
            _exec_id: &str,
        ) -> Result<Self::Group, ProcessGroupTrackerError> {
            Err(ProcessGroupTrackerError("invalid name".to_string()))
        }
    }
}
