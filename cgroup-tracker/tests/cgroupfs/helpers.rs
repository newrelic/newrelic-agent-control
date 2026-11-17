//! Helpers for the cgroupfs tests.

use cgroup_tracker::{CgroupName, CgroupTracker, Hierarchy};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Private v1 hierarchy mounted by the tests, so the one used by systemd is never touched.
const V1_HIERARCHY_NAME: &str = "nr-ac-test";
/// Directory inside a premounted [V1_HIERARCHY_NAME] hierarchy, writable by the test user.
/// When set, v1 cases use it instead of mounting, and never unmount.
pub const V1_BASE_ENV: &str = "CGROUP_TRACKER_V1_BASE";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Cgroup base private to a single test, so tests can run in parallel.
///
/// On drop, every cgroup handed out by [TestBase::cgroup] is killed and removed, and the v1
/// hierarchy is unmounted, so a failing test doesn't leave processes behind.
pub struct TestBase {
    tracker: CgroupTracker,
    names: RefCell<Vec<CgroupName>>,
    /// Mount point of the v1 hierarchy.
    v1_mount: Option<TempDir>,
}

impl TestBase {
    /// For v2, the base is a child of the cgroup the test process runs in. For v1, it is a child
    /// of the root of a freshly mounted named hierarchy.
    pub fn new(hierarchy: Hierarchy, test_name: &str) -> Self {
        let unique = format!("test-{test_name}-{hierarchy:?}-{}", std::process::id());
        let (base, v1_mount) = match hierarchy {
            Hierarchy::V2 => {
                let detected = CgroupTracker::detect().expect("detecting the test process cgroup");
                assert_eq!(
                    detected.hierarchy(),
                    Hierarchy::V2,
                    "cgroupfs tests need a cgroup v2 host"
                );
                (detected.base().join(unique), None)
            }
            Hierarchy::V1 => match std::env::var_os(V1_BASE_ENV) {
                // Premounted hierarchy: nothing to mount, so no root is needed.
                Some(parent) => (PathBuf::from(parent).join(unique), None),
                None => {
                    let mount = TempDir::new().unwrap();
                    let options = format!("none,name={V1_HIERARCHY_NAME}");
                    run(Command::new("mount")
                        .args(["-t", "cgroup", "-o", &options, "cgroup"])
                        .arg(mount.path()));
                    (mount.path().join(unique), Some(mount))
                }
            },
        };
        Self {
            tracker: CgroupTracker::with_base(base, hierarchy),
            names: RefCell::default(),
            v1_mount,
        }
    }

    pub fn tracker(&self) -> &CgroupTracker {
        &self.tracker
    }

    /// Returns a cgroup name that is cleaned up when the base is dropped.
    pub fn cgroup(&self, name: &str) -> CgroupName {
        let name = CgroupName::try_from(name).unwrap();
        self.names.borrow_mut().push(name.clone());
        name
    }

    /// Spawns `sh -c <script>` inside the cgroup.
    pub fn spawn(&self, name: &CgroupName, script: &str) -> Child {
        let mut command = Command::new("sh");
        command
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.tracker.prepare(name, &mut command).unwrap();
        command.spawn().unwrap()
    }

    /// Waits until the cgroup has exactly `count` members and returns them sorted.
    pub fn wait_for_members(&self, name: &CgroupName, count: usize) -> Vec<i32> {
        let mut members = Vec::new();
        wait_until(&format!("{count} members in cgroup {name}"), || {
            members = self.tracker.members(name).unwrap();
            members.len() == count
        });
        members.sort_unstable();
        members
    }

    /// Whether the process belongs to the cgroup at `path`.
    pub fn is_in(&self, pid: i32, path: &Path) -> bool {
        let content = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap();
        self.is_cgroup(path, &content)
    }

    /// Whether `/proc/<pid>/cgroup` content places the process in the cgroup at `path`.
    pub fn is_cgroup(&self, path: &Path, proc_cgroup: &str) -> bool {
        let cgroup_path = self.cgroup_path(proc_cgroup);
        // Paths in `/proc/<pid>/cgroup` are relative to the hierarchy root, not the mount point.
        let relative = cgroup_path.strip_prefix("/").unwrap_or(&cgroup_path);
        relative.as_os_str() != "" && path.ends_with(relative)
    }

    /// Extracts the path of the tracker hierarchy from `/proc/<pid>/cgroup` content.
    pub fn cgroup_path(&self, proc_cgroup: &str) -> PathBuf {
        let controllers = match self.tracker.hierarchy() {
            Hierarchy::V2 => String::new(),
            Hierarchy::V1 => format!("name={V1_HIERARCHY_NAME}"),
        };
        proc_cgroup
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, ':');
                Some((parts.next()?, parts.next()?, parts.next()?))
            })
            .find(|(_, c, _)| *c == controllers)
            .map(|(_, _, path)| PathBuf::from(path))
            .unwrap_or_else(|| panic!("no `{controllers}` hierarchy in:\n{proc_cgroup}"))
    }
}

impl Drop for TestBase {
    fn drop(&mut self) {
        for name in self.names.take() {
            if let Err(err) = self.tracker.kill_and_remove(&name) {
                eprintln!("cleaning up cgroup {name}: {err}");
            }
        }
        if let Err(err) = fs::remove_dir(self.tracker.base())
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("removing {}: {err}", self.tracker.base().display());
        }
        if let Some(mount) = &self.v1_mount {
            run(Command::new("umount").arg(mount.path()));
        }
    }
}

/// Fields of `/proc/<pid>/stat`.
#[derive(Debug)]
pub struct Stat {
    pub state: char,
    pub ppid: i32,
    pub session: i32,
}

/// Reads `/proc/<pid>/stat`. `None` once the process is fully gone.
pub fn stat(pid: i32) -> Option<Stat> {
    let content = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is wrapped in parentheses and may contain spaces.
    let (_, rest) = content.rsplit_once(')')?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(Stat {
        state: fields.first()?.chars().next()?,
        ppid: fields.get(1)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
    })
}

/// Whether the process exists and is not a zombie.
pub fn is_alive(pid: i32) -> bool {
    stat(pid).is_some_and(|s| !matches!(s.state, 'Z' | 'X'))
}

/// Polls `condition` until it holds, panicking after [WAIT_TIMEOUT].
pub fn wait_until(description: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn run(command: &mut Command) {
    let status = command.status().unwrap();
    assert!(status.success(), "{command:?} failed with {status}");
}
