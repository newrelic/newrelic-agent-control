use crate::error::DetectError;
use std::io::BufRead;
use std::path::{Path, PathBuf};

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

/// A cgroup mount found in `/proc/self/mountinfo`.
struct CgroupMount {
    root: PathBuf,
    mount_point: PathBuf,
    /// Super options, which list the controllers (or `name=...`) of a v1 hierarchy.
    options: Vec<String>,
}

/// Resolves the hierarchy and the cgroup directory of the current process from the content of
/// `/proc/self/mountinfo` and `/proc/self/cgroup`.
///
/// The unified hierarchy is preferred, which also covers hybrid setups. For v1 only the
/// `name=systemd` hierarchy is supported, since it has no controllers and so no side effects.
pub(crate) fn detect_base(
    mountinfo: impl BufRead,
    self_cgroup: impl BufRead,
) -> Result<(PathBuf, Hierarchy), DetectError> {
    let (v2_mounts, v1_mounts) = parse_mountinfo(mountinfo);
    let memberships = parse_self_cgroup(self_cgroup);

    // Unified membership is `0::<path>`; it only counts if a cgroup2 mount exists.
    let unified_path = memberships
        .iter()
        .find(|m| m.is_unified())
        .map(|m| m.path.as_str())
        .filter(|_| !v2_mounts.is_empty());

    match unified_path {
        Some(path) => Ok((resolve_v2(&v2_mounts, path)?, Hierarchy::V2)),
        None => Ok((resolve_v1(&v1_mounts, &memberships)?, Hierarchy::V1)),
    }
}

/// The same hierarchy can be mounted several times with different roots. Use the most
/// specific mount whose root contains our cgroup.
fn resolve_v2(mounts: &[CgroupMount], path: &str) -> Result<PathBuf, DetectError> {
    let mount = mounts
        .iter()
        .filter(|m| Path::new(path).starts_with(&m.root))
        .max_by_key(|m| m.root.components().count())
        .unwrap_or(&mounts[0]);
    resolve(mount, path)
}

/// Uses the `name=systemd` hierarchy, failing if it is not mounted.
fn resolve_v1(mounts: &[CgroupMount], memberships: &[Membership]) -> Result<PathBuf, DetectError> {
    let mount = mounts
        .iter()
        .find(|m| m.options.iter().any(|o| o == "name=systemd"))
        .ok_or(DetectError::NoSupportedHierarchy)?;
    let membership = memberships
        .iter()
        .find(|m| {
            m.controllers
                .split(',')
                .any(|c| mount.options.iter().any(|o| o == c))
        })
        .ok_or(DetectError::NotInAnyHierarchy)?;
    resolve(mount, &membership.path)
}

/// Maps the process cgroup path to a directory under the mount, stripping the mount root
/// (non-trivial inside cgroup namespaces and containers).
fn resolve(mount: &CgroupMount, cgroup_path: &str) -> Result<PathBuf, DetectError> {
    let relative = Path::new(cgroup_path)
        .strip_prefix(&mount.root)
        .map_err(|_| DetectError::OutsideMountRoot {
            cgroup_path: cgroup_path.to_string(),
            mount_root: mount.root.clone(),
        })?;
    Ok(mount.mount_point.join(relative))
}

/// Returns every `cgroup2` mount and every v1 `cgroup` mount.
fn parse_mountinfo(mountinfo: impl BufRead) -> (Vec<CgroupMount>, Vec<CgroupMount>) {
    let mut v2 = Vec::new();
    let mut v1 = Vec::new();
    for line in mountinfo.lines().map_while(Result::ok) {
        // `<id> <parent> <dev> <root> <mount point> <opts> [optional...] - <fstype> <source> <super opts>`
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let left: Vec<&str> = left.split_whitespace().collect();
        let right: Vec<&str> = right.split_whitespace().collect();
        let (Some(root), Some(mount_point), Some(fstype)) =
            (left.get(3), left.get(4), right.first())
        else {
            continue;
        };
        let mount = CgroupMount {
            root: PathBuf::from(root),
            mount_point: PathBuf::from(mount_point),
            options: right
                .get(2)
                .map(|o| o.split(',').map(str::to_string).collect())
                .unwrap_or_default(),
        };
        match *fstype {
            "cgroup2" => v2.push(mount),
            "cgroup" => v1.push(mount),
            _ => {}
        }
    }
    (v2, v1)
}

/// A `<hierarchy id>:<controllers>:<path>` line of `/proc/self/cgroup`.
struct Membership {
    id: String,
    controllers: String,
    path: String,
}

impl Membership {
    /// The cgroup v2 entry has id `0` and no controllers.
    fn is_unified(&self) -> bool {
        self.id == "0" && self.controllers.is_empty()
    }
}

fn parse_self_cgroup(self_cgroup: impl BufRead) -> Vec<Membership> {
    self_cgroup
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let mut parts = line.splitn(3, ':');
            Some(Membership {
                id: parts.next()?.to_string(),
                controllers: parts.next()?.to_string(),
                path: parts.next()?.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    const HYBRID_MOUNTS: &str = "\
35 25 0:30 / /sys/fs/cgroup/unified rw - cgroup2 cgroup2 rw
36 25 0:31 / /sys/fs/cgroup/systemd rw - cgroup cgroup rw,xattr,name=systemd
";

    // Captured from an Ubuntu 22.04 host booted with cgroup v1 (legacy), inside a systemd scope.
    const V1_MOUNTS: &str = "\
38 37 0:33 / /sys/fs/cgroup/systemd rw,nosuid,nodev,noexec,relatime shared:10 - cgroup cgroup rw,xattr,release_agent=/lib/systemd/systemd-cgroups-agent,name=systemd
42 37 0:37 / /sys/fs/cgroup/memory rw,nosuid,nodev,noexec,relatime shared:15 - cgroup cgroup rw,memory
43 37 0:38 / /sys/fs/cgroup/freezer rw,nosuid,nodev,noexec,relatime shared:16 - cgroup cgroup rw,freezer
44 37 0:39 / /sys/fs/cgroup/rdma rw,nosuid,nodev,noexec,relatime shared:17 - cgroup cgroup rw,rdma
45 37 0:40 / /sys/fs/cgroup/net_cls,net_prio rw,nosuid,nodev,noexec,relatime shared:18 - cgroup cgroup rw,net_cls,net_prio
46 37 0:41 / /sys/fs/cgroup/hugetlb rw,nosuid,nodev,noexec,relatime shared:19 - cgroup cgroup rw,hugetlb
47 37 0:42 / /sys/fs/cgroup/cpuset rw,nosuid,nodev,noexec,relatime shared:20 - cgroup cgroup rw,cpuset
48 37 0:43 / /sys/fs/cgroup/blkio rw,nosuid,nodev,noexec,relatime shared:21 - cgroup cgroup rw,blkio
49 37 0:44 / /sys/fs/cgroup/pids rw,nosuid,nodev,noexec,relatime shared:22 - cgroup cgroup rw,pids
50 37 0:45 / /sys/fs/cgroup/cpu,cpuacct rw,nosuid,nodev,noexec,relatime shared:23 - cgroup cgroup rw,cpu,cpuacct
51 37 0:46 / /sys/fs/cgroup/misc rw,nosuid,nodev,noexec,relatime shared:24 - cgroup cgroup rw,misc
52 37 0:47 / /sys/fs/cgroup/devices rw,nosuid,nodev,noexec,relatime shared:25 - cgroup cgroup rw,devices
53 37 0:48 / /sys/fs/cgroup/perf_event rw,nosuid,nodev,noexec,relatime shared:26 - cgroup cgroup rw,perf_event
";
    const V1_CGROUP: &str = "\
13:perf_event:/
12:devices:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
11:misc:/
10:cpu,cpuacct:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
9:pids:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
8:blkio:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
7:cpuset:/
6:hugetlb:/
5:net_cls,net_prio:/
4:rdma:/
3:freezer:/
2:memory:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
1:name=systemd:/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope
";
    // Captured from an Ubuntu 24.04 host (cgroup v2), inside a systemd scope.
    const V2_MOUNTS: &str = "\
37 27 0:32 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:10 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot
";
    const V2_CGROUP: &str = "0::/system.slice/run-r22c0d05185894b7b863749c969f67a1c.scope\n";

    #[rstest]
    #[case::v1(
        V1_MOUNTS,
        V1_CGROUP,
        "/sys/fs/cgroup/systemd/system.slice/run-rcaccada4b18f46ebb5cf8b4e54d49ab1.scope",
        Hierarchy::V1
    )]
    #[case::v2(
        V2_MOUNTS,
        V2_CGROUP,
        "/sys/fs/cgroup/system.slice/run-r22c0d05185894b7b863749c969f67a1c.scope",
        Hierarchy::V2
    )]
    #[case::v2_namespace(V2_MOUNTS, "0::/\n", "/sys/fs/cgroup", Hierarchy::V2)]
    #[case::hybrid_prefers_v2(
        HYBRID_MOUNTS,
        "1:name=systemd:/system.slice/ac.service\n0::/system.slice/ac.service\n",
        "/sys/fs/cgroup/unified/system.slice/ac.service",
        Hierarchy::V2
    )]
    fn detects(
        #[case] mountinfo: &str,
        #[case] self_cgroup: &str,
        #[case] expected_base: &str,
        #[case] expected_hierarchy: Hierarchy,
    ) {
        let (base, hierarchy) = detect_base(mountinfo.as_bytes(), self_cgroup.as_bytes()).unwrap();
        assert_eq!(base, PathBuf::from(expected_base));
        assert_eq!(hierarchy, expected_hierarchy);
    }

    #[test]
    fn strips_mount_root() {
        let mountinfo = "35 25 0:30 /ns/root /sys/fs/cgroup rw - cgroup2 cgroup2 rw\n";
        let (base, _) = detect_base(mountinfo.as_bytes(), "0::/ns/root/leaf\n".as_bytes()).unwrap();
        assert_eq!(base, PathBuf::from("/sys/fs/cgroup/leaf"));
    }

    #[rstest]
    #[case::skips_non_matching_root(
        "\
35 25 0:30 /ns/other /mnt/other rw - cgroup2 cgroup2 rw
36 25 0:30 /ns/root /sys/fs/cgroup rw - cgroup2 cgroup2 rw
",
        "/sys/fs/cgroup/leaf"
    )]
    #[case::prefers_most_specific_root(
        "\
35 25 0:30 / /mnt/full rw - cgroup2 cgroup2 rw
36 25 0:30 /ns/root /sys/fs/cgroup rw - cgroup2 cgroup2 rw
",
        "/sys/fs/cgroup/leaf"
    )]
    fn picks_matching_v2_mount(#[case] mountinfo: &str, #[case] expected_base: &str) {
        let (base, hierarchy) =
            detect_base(mountinfo.as_bytes(), "0::/ns/root/leaf\n".as_bytes()).unwrap();
        assert_eq!(base, PathBuf::from(expected_base));
        assert_eq!(hierarchy, Hierarchy::V2);
    }

    #[test]
    fn no_mount() {
        assert_matches!(
            detect_base("".as_bytes(), "0::/\n".as_bytes()),
            Err(DetectError::NoSupportedHierarchy)
        );
    }

    #[test]
    fn cgroup_outside_mount_root() {
        let mountinfo = "35 25 0:30 /ns/root /sys/fs/cgroup rw - cgroup2 cgroup2 rw\n";
        assert_matches!(
            detect_base(mountinfo.as_bytes(), "0::/other\n".as_bytes()),
            Err(DetectError::OutsideMountRoot { cgroup_path, mount_root }) => {
                assert_eq!(cgroup_path, "/other");
                assert_eq!(mount_root, PathBuf::from("/ns/root"));
            }
        );
    }

    #[test]
    fn v1_without_systemd_mount() {
        let mountinfo: String = V1_MOUNTS
            .lines()
            .filter(|l| !l.contains("name=systemd"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_matches!(
            detect_base(mountinfo.as_bytes(), V1_CGROUP.as_bytes()),
            Err(DetectError::NoSupportedHierarchy)
        );
    }

    #[test]
    fn no_membership() {
        assert_matches!(
            detect_base(V1_MOUNTS.as_bytes(), "".as_bytes()),
            Err(DetectError::NotInAnyHierarchy)
        );
    }
}
