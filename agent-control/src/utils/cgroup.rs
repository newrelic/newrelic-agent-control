//! Linux cgroup tracking of sub-agent process trees.
//!
//! Every executable runs in its own cgroup, created under Agent Control's own cgroup. The cgroup
//! outlives Agent Control and its membership is maintained by the kernel, so a restarted Agent
//! Control finds the process tree of a running executable by name, with no persisted state.
//!
//! Only one hierarchy is used, purely for tracking (no controllers, no limits): the cgroup v2
//! mount if present (on hybrid hosts too), otherwise the cgroup v1 `name=systemd` hierarchy.
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use procfs::ProcessCGroups;
use procfs::process::{MountInfos, Process, Stat};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

const CGROUP_NAME_PREFIX: &str = "nr-ac-";
const PROCS_FILE: &str = "cgroup.procs";
const KILL_FILE: &str = "cgroup.kill";
const KILL_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Error detecting or preparing the cgroup hierarchy used to track sub-agents.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct CgroupError(String);

/// Kind of cgroup hierarchy a mount point belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hierarchy {
    V2,
    V1Systemd,
}

/// Directory, inside the tracking hierarchy, under which the cgroup of every executable lives.
#[derive(Debug, Clone, PartialEq)]
pub struct CgroupTracker {
    base: PathBuf,
}

impl CgroupTracker {
    /// Detects the tracking hierarchy and Agent Control's own cgroup in it.
    pub fn detect() -> Result<Self, CgroupError> {
        let myself =
            Process::myself().map_err(|e| CgroupError(format!("reading /proc/self: {e}")))?;
        let mountinfo = myself
            .mountinfo()
            .map_err(|e| CgroupError(format!("reading /proc/self/mountinfo: {e}")))?;
        let cgroups = myself
            .cgroups()
            .map_err(|e| CgroupError(format!("reading /proc/self/cgroup: {e}")))?;
        Self::from_proc(&mountinfo, &cgroups)
    }

    fn from_proc(mountinfo: &MountInfos, cgroups: &ProcessCGroups) -> Result<Self, CgroupError> {
        [Hierarchy::V2, Hierarchy::V1Systemd]
            .into_iter()
            .find_map(|hierarchy| {
                let cgroup_path = self_cgroup_path(cgroups, hierarchy)?;
                mountinfo
                    .iter()
                    .filter(|m| mount_hierarchy(m) == Some(hierarchy))
                    .find_map(|m| {
                        let relative = Path::new(cgroup_path).strip_prefix(&m.root).ok()?;
                        Some(Self {
                            base: m.mount_point.join(relative),
                        })
                    })
            })
            .ok_or_else(|| {
                CgroupError(
                    "no usable cgroup hierarchy found: neither a cgroup v2 mount nor the cgroup v1 \
                     'name=systemd' hierarchy contains the cgroup of this process"
                        .to_string(),
                )
            })
    }

    /// Checks that cgroups can be created and removed under Agent Control's own cgroup.
    /// It creates and deletes a probe cgroup directory and errors on failure
    pub fn ensure_usable(&self) -> Result<(), CgroupError> {
        let probe = self
            .base
            .join(format!("{CGROUP_NAME_PREFIX}probe-{}", std::process::id()));
        fs::create_dir(&probe)
            .map_err(|e| CgroupError(format!("creating cgroup {}: {e}", probe.display())))?;
        fs::remove_dir(&probe)
            .map_err(|e| CgroupError(format!("removing cgroup {}: {e}", probe.display())))
    }

    /// The cgroup of the given executable. It is not created until [`ExecutableCgroup::create`].
    pub fn executable_cgroup(&self, agent_id: &str, exec_id: &str) -> ExecutableCgroup {
        let name = format!(
            "{CGROUP_NAME_PREFIX}{}-{}",
            sanitize(agent_id),
            sanitize(exec_id)
        );
        ExecutableCgroup {
            path: self.base.join(name),
        }
    }

    /// Removes the executable cgroups that have no processes. Non-empty ones are left untouched:
    /// removing a cgroup with members fails.
    pub fn sweep_empty(&self) {
        let Ok(entries) = fs::read_dir(&self.base) else {
            return;
        };
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(CGROUP_NAME_PREFIX)
            {
                let _ = fs::remove_dir(entry.path());
            }
        }
    }
}

/// Cgroup holding the process tree of a single executable.
#[derive(Debug, Clone)]
pub struct ExecutableCgroup {
    path: PathBuf,
}

impl ExecutableCgroup {
    /// Whether the cgroup exists.
    pub fn exists(&self) -> bool {
        self.path.is_dir()
    }

    /// Creates the cgroup if it doesn't exist yet.
    pub fn create(&self) -> io::Result<()> {
        match fs::create_dir(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
            _ => Ok(()),
        }
    }

    /// Makes `command` start inside this cgroup: the child writes its own pid to `cgroup.procs`
    /// between fork and exec, so it is never running outside of it. The cgroup must exist.
    pub fn attach(&self, command: &mut Command) -> io::Result<()> {
        let procs: File = OpenOptions::new()
            .write(true)
            .open(self.path.join(PROCS_FILE))?;
        // SAFETY: the closure only performs a `write` syscall on a file descriptor opened
        // beforehand (no allocation, no locks), which is async-signal-safe.
        unsafe {
            command.pre_exec(move || (&procs).write_all(b"0"));
        }
        Ok(())
    }

    /// Live (non-zombie) processes in the cgroup, descendants included.
    pub fn live_pids(&self) -> io::Result<Vec<u32>> {
        let content = match fs::read_to_string(self.path.join(PROCS_FILE)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            other => other?,
        };
        Ok(content
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .filter(|pid| proc_stat(*pid).is_some_and(|stat| stat.state != 'Z'))
            .collect())
    }

    /// The live member leading its own process group, i.e. the process the cgroup was created for.
    pub fn leader(&self) -> io::Result<Option<u32>> {
        Ok(self
            .live_pids()?
            .into_iter()
            .find(|pid| proc_stat(*pid).is_some_and(|stat| stat.pgrp == *pid as i32)))
    }

    /// Kills every process in the cgroup and returns once none is left alive.
    ///
    /// `cgroup.kill` (cgroup v2, Linux 5.14+) takes the tree down atomically. Elsewhere, members
    /// are signaled repeatedly until the cgroup is empty, which also catches processes forked
    /// while signaling.
    pub fn kill_all(&self) -> io::Result<()> {
        // /sys/fs/cgroup/system.slice/newrelic-agent-control.service/nr-ac-<agent_id>-<exec_id>/cgroup.kill
        // echo 1 > <dir>/cgroup.kill kills everything in it, as root. cgroupsV2
        let kill_file = self.path.join(KILL_FILE);
        if kill_file.exists() {
            let _ = fs::write(kill_file, "1");
        }
        let deadline = Instant::now() + KILL_TIMEOUT;
        loop {
            let pids = self.live_pids()?;
            if pids.is_empty() {
                return Ok(());
            }
            // If no killall, we kill pid by pid
            for pid in pids {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "processes still alive in cgroup {} after {KILL_TIMEOUT:?}",
                    self.path.display()
                )));
            }
            sleep(POLL_INTERVAL);
        }
    }

    /// Removes the cgroup, which must have no members. A missing cgroup is not an error.
    pub fn remove(&self) -> io::Result<()> {
        match fs::remove_dir(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Keeps only characters that are safe in a directory name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// State and process group of `pid`, if it still exists.
fn proc_stat(pid: u32) -> Option<Stat> {
    Process::new(pid as i32).ok()?.stat().ok()
}

/// The tracking hierarchy a mount belongs to, if any.
fn mount_hierarchy(mount: &procfs::process::MountInfo) -> Option<Hierarchy> {
    match mount.fs_type.as_str() {
        "cgroup2" => Some(Hierarchy::V2),
        "cgroup" if mount.super_options.contains_key("name=systemd") => Some(Hierarchy::V1Systemd),
        _ => None,
    }
}

/// Cgroup path of this process in the given hierarchy.
fn self_cgroup_path(cgroups: &ProcessCGroups, hierarchy: Hierarchy) -> Option<&str> {
    cgroups
        .0
        .iter()
        .find(|cgroup| match hierarchy {
            Hierarchy::V2 => cgroup.controllers.is_empty(),
            Hierarchy::V1Systemd => cgroup.controllers.iter().any(|c| c == "name=systemd"),
        })
        .map(|cgroup| cgroup.pathname.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use procfs::FromBufRead;
    use std::io::Cursor;
    use std::process::Stdio;

    fn tracker_from(mountinfo: &str, cgroups: &str) -> Result<CgroupTracker, CgroupError> {
        let mountinfo = MountInfos::from_buf_read(Cursor::new(mountinfo)).unwrap();
        let cgroups = ProcessCGroups::from_buf_read(Cursor::new(cgroups)).unwrap();
        CgroupTracker::from_proc(&mountinfo, &cgroups)
    }

    const V2_MOUNTINFO: &str = "\
25 20 0:22 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:4 - cgroup2 cgroup2 rw,nsdelegate
26 20 0:23 / /sys rw shared:5 - sysfs sysfs rw";
    const V2_CGROUP: &str = "0::/system.slice/newrelic-agent-control.service\n";

    const HYBRID_MOUNTINFO: &str = "\
24 23 0:21 / /sys/fs/cgroup/unified rw,nosuid,nodev,noexec,relatime shared:5 - cgroup2 cgroup2 rw,nsdelegate
25 23 0:22 / /sys/fs/cgroup/systemd rw,nosuid,nodev,noexec,relatime shared:6 - cgroup cgroup rw,xattr,name=systemd
26 23 0:23 / /sys/fs/cgroup/cpu,cpuacct rw,nosuid,nodev,noexec,relatime shared:7 - cgroup cgroup rw,cpu,cpuacct";
    const HYBRID_CGROUP: &str = "\
12:cpu,cpuacct:/system.slice/newrelic-agent-control.service
1:name=systemd:/system.slice/newrelic-agent-control.service
0::/system.slice/newrelic-agent-control.service
";

    const V1_MOUNTINFO: &str = "\
25 23 0:22 / /sys/fs/cgroup/systemd rw,nosuid,nodev,noexec,relatime shared:6 - cgroup cgroup rw,xattr,name=systemd
26 23 0:23 / /sys/fs/cgroup/cpu,cpuacct rw,nosuid,nodev,noexec,relatime shared:7 - cgroup cgroup rw,cpu,cpuacct";
    const V1_CGROUP: &str = "\
5:cpu,cpuacct:/system.slice/newrelic-agent-control.service
1:name=systemd:/system.slice/newrelic-agent-control.service
";

    #[test]
    fn detects_cgroup_v2() {
        let tracker = tracker_from(V2_MOUNTINFO, V2_CGROUP).unwrap();
        assert_eq!(
            tracker.base,
            Path::new("/sys/fs/cgroup/system.slice/newrelic-agent-control.service")
        );
    }

    #[test]
    fn hybrid_prefers_the_unified_hierarchy() {
        let tracker = tracker_from(HYBRID_MOUNTINFO, HYBRID_CGROUP).unwrap();
        assert_eq!(
            tracker.base,
            Path::new("/sys/fs/cgroup/unified/system.slice/newrelic-agent-control.service")
        );
    }

    #[test]
    fn cgroup_v1_falls_back_to_the_systemd_hierarchy() {
        let tracker = tracker_from(V1_MOUNTINFO, V1_CGROUP).unwrap();
        assert_eq!(
            tracker.base,
            Path::new("/sys/fs/cgroup/systemd/system.slice/newrelic-agent-control.service")
        );
    }

    #[test]
    fn mount_root_is_stripped_from_the_cgroup_path() {
        let mountinfo = "30 20 0:25 /system.slice /sys/fs/cgroup rw - cgroup2 cgroup2 rw";
        let tracker = tracker_from(mountinfo, "0::/system.slice/newrelic.service").unwrap();
        assert_eq!(tracker.base, Path::new("/sys/fs/cgroup/newrelic.service"));
    }

    #[test]
    fn no_usable_hierarchy_is_an_error() {
        let mountinfo = "26 20 0:23 / /sys/fs/cgroup/cpu rw - cgroup cgroup rw,cpu";
        assert!(tracker_from(mountinfo, "5:cpu:/").is_err());
        assert!(tracker_from("", "").is_err());
    }

    #[test]
    fn executable_cgroup_name_is_sanitized() {
        let tracker = CgroupTracker {
            base: PathBuf::from("/base"),
        };
        let cgroup = tracker.executable_cgroup("agent-1", "exec/../id 1");
        assert_eq!(cgroup.path, Path::new("/base/nr-ac-agent-1-exec_.._id_1"));
    }

    #[test]
    fn sweep_removes_only_empty_executable_cgroups() {
        let dir = tempfile::tempdir().unwrap();
        let tracker = CgroupTracker {
            base: dir.path().to_path_buf(),
        };
        fs::create_dir(dir.path().join("nr-ac-a-b")).unwrap();
        fs::create_dir(dir.path().join("other")).unwrap();
        // A directory with content stands in for a cgroup with members (rmdir fails).
        fs::create_dir(dir.path().join("nr-ac-busy-x")).unwrap();
        fs::write(dir.path().join("nr-ac-busy-x").join(PROCS_FILE), "1").unwrap();

        tracker.sweep_empty();

        assert!(!dir.path().join("nr-ac-a-b").exists());
        assert!(dir.path().join("other").exists());
        assert!(dir.path().join("nr-ac-busy-x").exists());
    }

    #[test]
    fn proc_stat_reads_state_and_group_of_the_current_process() {
        let stat = proc_stat(std::process::id()).unwrap();
        assert_ne!(stat.state, 'Z');
        assert!(stat.pgrp > 0);
    }

    #[test]
    fn live_pids_ignores_missing_cgroup_dead_and_non_numeric_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cgroup = ExecutableCgroup {
            path: dir.path().join("missing"),
        };
        assert!(cgroup.live_pids().unwrap().is_empty());

        let cgroup = ExecutableCgroup {
            path: dir.path().to_path_buf(),
        };
        let me = std::process::id();
        fs::write(dir.path().join(PROCS_FILE), format!("{me}\nabc\n4194999\n")).unwrap();
        assert_eq!(cgroup.live_pids().unwrap(), vec![me]);
    }

    #[test]
    fn remove_missing_cgroup_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cgroup = ExecutableCgroup {
            path: dir.path().join("missing"),
        };
        cgroup.remove().unwrap();
    }

    /// Needs write access to the cgroup hierarchy (root or a delegated cgroup).
    #[test]
    #[ignore = "needs write access to the cgroup hierarchy"]
    fn tracks_and_kills_a_process_tree_in_a_real_cgroup() {
        let tracker = CgroupTracker::detect().unwrap();
        tracker.ensure_usable().unwrap();
        let cgroup = tracker.executable_cgroup("cgroup-test", "tree");
        cgroup.create().unwrap();

        // A shell leading its group, with a background grandchild.
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & wait"])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cgroup.attach(&mut command).unwrap();
        let mut child = command.spawn().unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while cgroup.live_pids().unwrap().len() < 2 {
            assert!(Instant::now() < deadline, "tree never showed up in cgroup");
            sleep(POLL_INTERVAL);
        }
        assert_eq!(cgroup.leader().unwrap(), Some(child.id()));

        cgroup.kill_all().unwrap();
        child.wait().unwrap();
        assert!(cgroup.live_pids().unwrap().is_empty());
        cgroup.remove().unwrap();
        assert!(!cgroup.path.exists());
    }

    /// Needs write access to the cgroup hierarchy (root or a delegated cgroup). `sweep_empty`
    /// removes every empty `nr-ac-*` cgroup, so run it apart from the other real-cgroup tests
    /// (`--test-threads=1`).
    #[test]
    #[ignore = "needs write access to the cgroup hierarchy"]
    fn sweep_empty_keeps_cgroups_with_processes_in_a_real_cgroup() {
        let tracker = CgroupTracker::detect().unwrap();
        tracker.ensure_usable().unwrap();
        let busy = tracker.executable_cgroup("cgroup-test", "sweep-busy");
        let empty = tracker.executable_cgroup("cgroup-test", "sweep-empty");
        busy.create().unwrap();
        empty.create().unwrap();
        let mut command = Command::new("sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        busy.attach(&mut command).unwrap();
        let mut child = command.spawn().unwrap();

        tracker.sweep_empty();

        assert!(busy.exists(), "a cgroup with a process must not be removed");
        assert!(!empty.exists(), "an empty cgroup must be removed");

        busy.kill_all().unwrap();
        child.wait().unwrap();
        tracker.sweep_empty();

        assert!(!busy.exists(), "the cgroup is removed once it is empty");
    }
}
