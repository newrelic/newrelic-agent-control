use crate::{CgroupError, CgroupName, Hierarchy};
use std::path::{Path, PathBuf};

/// Creates, attaches to, kills and removes the cgroups of sub-agent executables.
#[derive(Debug, Clone)]
pub struct CgroupTracker {
    base: PathBuf,
    hierarchy: Hierarchy,
}

impl CgroupTracker {
    /// Detects the hierarchy and base cgroup from `/proc/self`.
    pub fn detect() -> Result<Self, CgroupError> {
        todo!("parse /proc/self/mountinfo and /proc/self/cgroup")
    }

    /// Uses an explicit base directory. Meant for tests and future configuration.
    pub fn with_base(base: impl Into<PathBuf>, hierarchy: Hierarchy) -> Self {
        Self {
            base: base.into(),
            hierarchy,
        }
    }

    /// Base directory under which the tracker creates its cgroups.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Hierarchy kind the tracker operates on.
    pub fn hierarchy(&self) -> Hierarchy {
        self.hierarchy
    }

    /// Absolute path of the cgroup for `name`.
    pub fn path_of(&self, name: &CgroupName) -> PathBuf {
        self.base.join(name.relative_path())
    }

    /// Creates the cgroup. Idempotent.
    pub fn create(&self, _name: &CgroupName) -> Result<(), CgroupError> {
        todo!()
    }

    /// Configures `command` so the child joins the cgroup before `exec` and leads its own
    /// process group, keeping both invariants in a single place.
    pub fn prepare(
        &self,
        _name: &CgroupName,
        _command: &mut std::process::Command,
    ) -> Result<(), CgroupError> {
        todo!()
    }

    /// Kills every process in the cgroup (and its descendants), then removes it.
    pub fn kill_and_remove(&self, _name: &CgroupName) -> Result<(), CgroupError> {
        todo!()
    }

    /// Removes empty cgroups left by a previous run. Startup-only: it can race with
    /// `create` + `prepare`.
    pub fn sweep_empty(&self) -> Result<(), CgroupError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_of_joins_base_and_name() {
        let tracker = CgroupTracker::with_base("/sys/fs/cgroup/test", Hierarchy::V2);
        assert_eq!(
            tracker.path_of(&CgroupName::new("agent", "exec")),
            PathBuf::from("/sys/fs/cgroup/test/nr-ac-agent/exec")
        );
    }
}
