//! Process tracking on unix targets.

use super::{ProcessGroupTracker, ProcessGroupTrackerError, TrackedProcessGroup};
use crate::agent_control::agent_id::AgentID;
use cgroup_tracker::CgroupName;
use std::process::{Child, Command};

/// Tracker chosen at startup from the experimental configuration.
#[derive(Debug, Clone)]
pub enum UnixProcessGroupTracker {
    Passthrough(PassthroughProcessGroupTracker),
    Cgroup(CgroupTracker),
}

impl Default for UnixProcessGroupTracker {
    fn default() -> Self {
        Self::Passthrough(PassthroughProcessGroupTracker)
    }
}

impl ProcessGroupTracker for UnixProcessGroupTracker {
    type Group = UnixProcessGroup;

    fn group(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
    ) -> Result<UnixProcessGroup, ProcessGroupTrackerError> {
        match self {
            Self::Passthrough(tracker) => tracker
                .group(agent_id, exec_id)
                .map(UnixProcessGroup::Passthrough),
            Self::Cgroup(tracker) => tracker
                .group(agent_id, exec_id)
                .map(UnixProcessGroup::Cgroup),
        }
    }
}

#[derive(Debug)]
pub enum UnixProcessGroup {
    Passthrough(PassthroughProcessGroup),
    Cgroup(CgroupProcessGroup),
}

impl TrackedProcessGroup for UnixProcessGroup {
    fn spawn(&self, command: Command) -> Result<Child, ProcessGroupTrackerError> {
        match self {
            Self::Passthrough(group) => group.spawn(command),
            Self::Cgroup(group) => group.spawn(command),
        }
    }

    fn kill(self) -> Result<(), ProcessGroupTrackerError> {
        match self {
            Self::Passthrough(group) => group.kill(),
            Self::Cgroup(group) => group.kill(),
        }
    }
}

/// Spawns the processes of every executable as they are, without keeping them in any group.
#[derive(Debug, Clone, Default)]
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

/// Keeps the processes of every executable in a cgroup of its own.
#[derive(Debug, Clone)]
pub struct CgroupTracker(cgroup_tracker::CgroupTracker);

impl CgroupTracker {
    pub fn new(tracker: cgroup_tracker::CgroupTracker) -> Self {
        Self(tracker)
    }

    pub fn detect() -> Result<Self, ProcessGroupTrackerError> {
        cgroup_tracker::CgroupTracker::detect()
            .map(Self::new)
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }
}

impl ProcessGroupTracker for CgroupTracker {
    type Group = CgroupProcessGroup;

    /// An invalid executable id fails only this executable, it never gets a cgroup.
    fn group(
        &self,
        agent_id: &AgentID,
        exec_id: &str,
    ) -> Result<CgroupProcessGroup, ProcessGroupTrackerError> {
        // `<agent_id>_<exec_id>`: agent ids contain no `_`, so the agent can be told apart from the
        // executable, and the `_` keeps the name away from the names the kernel reserves.
        let name = CgroupName::try_from(format!("{agent_id}_{exec_id}"))
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))?;
        Ok(CgroupProcessGroup {
            tracker: self.0.clone(),
            name,
        })
    }
}

/// The cgroup of an executable.
#[derive(Debug)]
pub struct CgroupProcessGroup {
    tracker: cgroup_tracker::CgroupTracker,
    name: CgroupName,
}

impl TrackedProcessGroup for CgroupProcessGroup {
    fn spawn(&self, command: Command) -> Result<Child, ProcessGroupTrackerError> {
        self.tracker
            .cgroup(&self.name)
            .spawn(command)
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }

    fn kill(self) -> Result<(), ProcessGroupTrackerError> {
        self.tracker
            .cgroup(&self.name)
            .kill_and_remove()
            .map_err(|err| ProcessGroupTrackerError(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgroup_tracker::CgroupVersion;
    use rstest::rstest;
    use tempfile::TempDir;

    fn agent_id() -> AgentID {
        AgentID::try_from("agent-a").unwrap()
    }

    #[test]
    fn default_tracker_is_passthrough() {
        assert!(matches!(
            UnixProcessGroupTracker::default(),
            UnixProcessGroupTracker::Passthrough(_)
        ));
    }

    #[test]
    fn passthrough_tracker_gives_passthrough_groups() {
        let tracker = UnixProcessGroupTracker::default();

        let group = tracker.group(&agent_id(), "exec-b").unwrap();

        assert!(matches!(group, UnixProcessGroup::Passthrough(_)));
    }

    #[test]
    fn cgroup_tracker_gives_cgroup_groups() {
        let dir = TempDir::new().unwrap();
        let tracker = UnixProcessGroupTracker::Cgroup(CgroupTracker::new(
            cgroup_tracker::CgroupTracker::from_parts(dir.path(), CgroupVersion::V1),
        ));

        let group = tracker.group(&agent_id(), "exec-b").unwrap();

        assert!(matches!(group, UnixProcessGroup::Cgroup(_)));
    }

    #[test]
    fn cgroup_name_joins_agent_and_executable() {
        let dir = TempDir::new().unwrap();
        let tracker = CgroupTracker::new(cgroup_tracker::CgroupTracker::from_parts(
            dir.path(),
            CgroupVersion::V1,
        ));

        let group = tracker.group(&agent_id(), "exec-b").unwrap();

        assert_eq!(group.name.to_string(), "agent-a_exec-b");
    }

    #[rstest]
    #[case::slash("a/b")]
    #[case::space("a b")]
    fn cgroup_group_fails_for_an_invalid_executable_id(#[case] exec_id: &str) {
        let dir = TempDir::new().unwrap();
        let tracker = CgroupTracker::new(cgroup_tracker::CgroupTracker::from_parts(
            dir.path(),
            CgroupVersion::V1,
        ));

        assert!(tracker.group(&agent_id(), exec_id).is_err());
    }

    #[test]
    fn passthrough_group_spawns_the_command_and_kill_succeeds() {
        let group = PassthroughProcessGroup;

        let mut child = group.spawn(Command::new("true")).unwrap();
        child.wait().unwrap();

        assert!(group.kill().is_ok());
    }
}
