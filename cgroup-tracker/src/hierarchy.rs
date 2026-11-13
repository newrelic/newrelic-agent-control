use crate::CgroupError;
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
/// The unified hierarchy is preferred, which also covers hybrid setups. For v1 the
/// `name=systemd` hierarchy is preferred, falling back to the first cgroup mount.
pub(crate) fn detect_base(
    mountinfo: impl BufRead,
    self_cgroup: impl BufRead,
) -> Result<(PathBuf, Hierarchy), CgroupError> {
    let (v2_mount, v1_mounts) = parse_mountinfo(mountinfo);
    let memberships = parse_self_cgroup(self_cgroup);

    if let Some(mount) = v2_mount
        && let Some((_, _, path)) = memberships
            .iter()
            .find(|(id, ctrl, _)| *id == "0" && ctrl.is_empty())
    {
        return Ok((resolve(&mount, path)?, Hierarchy::V2));
    }

    let mount = v1_mounts
        .iter()
        .find(|m| m.options.iter().any(|o| o == "name=systemd"))
        .or(v1_mounts.first())
        .ok_or_else(|| CgroupError::Detect("no cgroup mount found".to_string()))?;
    let path = memberships
        .iter()
        .find(|(_, ctrl, _)| {
            ctrl.split(',')
                .any(|c| mount.options.iter().any(|o| o == c))
        })
        .map(|(_, _, path)| path)
        .ok_or_else(|| {
            CgroupError::Detect(
                "process is not a member of any mounted cgroup hierarchy".to_string(),
            )
        })?;
    Ok((resolve(mount, path)?, Hierarchy::V1))
}

/// Maps the process cgroup path to a directory under the mount, stripping the mount root
/// (non-trivial inside cgroup namespaces and containers).
fn resolve(mount: &CgroupMount, cgroup_path: &str) -> Result<PathBuf, CgroupError> {
    let relative = Path::new(cgroup_path)
        .strip_prefix(&mount.root)
        .map_err(|_| {
            CgroupError::Detect(format!(
                "cgroup path `{cgroup_path}` is outside the mount root `{}`",
                mount.root.display()
            ))
        })?;
    Ok(mount.mount_point.join(relative))
}

/// Returns the first `cgroup2` mount and every v1 `cgroup` mount.
fn parse_mountinfo(mountinfo: impl BufRead) -> (Option<CgroupMount>, Vec<CgroupMount>) {
    let mut v2 = None;
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
            "cgroup2" if v2.is_none() => v2 = Some(mount),
            "cgroup" => v1.push(mount),
            _ => {}
        }
    }
    (v2, v1)
}

/// Parses `<hierarchy id>:<controllers>:<path>` lines.
fn parse_self_cgroup(self_cgroup: impl BufRead) -> Vec<(String, String, String)> {
    self_cgroup
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let mut parts = line.splitn(3, ':');
            Some((
                parts.next()?.to_string(),
                parts.next()?.to_string(),
                parts.next()?.to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    const V2_MOUNTS: &str =
        "35 25 0:30 / /sys/fs/cgroup rw,nosuid - cgroup2 cgroup2 rw,nsdelegate\n";
    const V1_MOUNTS: &str = "\
36 25 0:31 / /sys/fs/cgroup/systemd rw - cgroup cgroup rw,xattr,name=systemd
37 25 0:32 / /sys/fs/cgroup/cpu,cpuacct rw - cgroup cgroup rw,cpu,cpuacct
";
    const HYBRID_MOUNTS: &str = "\
35 25 0:30 / /sys/fs/cgroup/unified rw - cgroup2 cgroup2 rw
36 25 0:31 / /sys/fs/cgroup/systemd rw - cgroup cgroup rw,xattr,name=systemd
";

    #[rstest]
    #[case::v2(
        V2_MOUNTS,
        "0::/system.slice/ac.service\n",
        "/sys/fs/cgroup/system.slice/ac.service",
        Hierarchy::V2
    )]
    #[case::v2_namespace(V2_MOUNTS, "0::/\n", "/sys/fs/cgroup", Hierarchy::V2)]
    #[case::hybrid_prefers_v2(
        HYBRID_MOUNTS,
        "1:name=systemd:/system.slice/ac.service\n0::/system.slice/ac.service\n",
        "/sys/fs/cgroup/unified/system.slice/ac.service",
        Hierarchy::V2
    )]
    #[case::v1_systemd(
        V1_MOUNTS,
        "3:cpu,cpuacct:/user\n1:name=systemd:/system.slice/ac.service\n",
        "/sys/fs/cgroup/systemd/system.slice/ac.service",
        Hierarchy::V1
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
    #[case::no_mount("", "0::/\n")]
    #[case::outside_root(
        "35 25 0:30 /ns/root /sys/fs/cgroup rw - cgroup2 cgroup2 rw\n",
        "0::/other\n"
    )]
    #[case::no_membership(V1_MOUNTS, "")]
    fn missing_hierarchy(#[case] mountinfo: &str, #[case] self_cgroup: &str) {
        assert_matches!(
            detect_base(mountinfo.as_bytes(), self_cgroup.as_bytes()),
            Err(CgroupError::Detect(_))
        );
    }
}
