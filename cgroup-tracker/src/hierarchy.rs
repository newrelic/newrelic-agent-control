/// Kind of cgroup hierarchy the tracker operates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hierarchy {
    /// Unified hierarchy. Supports `cgroup.kill`.
    V2,
    /// Legacy (or hybrid named) hierarchy. Killing relies on signalling the members.
    V1,
}

impl Hierarchy {
    /// Whether the hierarchy provides the `cgroup.kill` interface file.
    pub fn supports_cgroup_kill(self) -> bool {
        matches!(self, Hierarchy::V2)
    }
}

// TODO: parsing of `/proc/self/mountinfo` and `/proc/self/cgroup` content (v2, hybrid, v1,
// mount-root stripping, cgroup namespaces). Take `impl BufRead` so fixtures can drive it.
