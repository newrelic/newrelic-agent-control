//! Tests against the real cgroupfs. They need root and a writable cgroupfs, so they are ignored
//! by default:
//!
//! ```text
//! cargo test -p cgroup-tracker --test cgroupfs --no-run   # prints the test binary path
//! sudo <test binary path> --ignored
//! ```
//!
//! Every test runs against both hierarchies. The v2 case uses a scratch cgroup under the one of
//! the test process and is skipped on hosts without the unified hierarchy. The v1 case mounts a
//! private named hierarchy (`none,name=...`), which works regardless of the host hierarchy.

#![cfg(target_os = "linux")]

use cgroup_tracker::{CgroupName, CgroupTracker, Hierarchy};
use nix::mount::{MsFlags, mount, umount};
use nix::unistd::{Pid, getpgid};
use rstest::rstest;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const AGENT: &str = "it";
const POLL_TIMEOUT: Duration = Duration::from_secs(10);

/// Isolated cgroup tree for a single test, torn down on drop.
struct Scratch {
    tracker: CgroupTracker,
    /// Directory holding everything the test created.
    root: PathBuf,
    /// Mount directory of the private v1 hierarchy, if any.
    v1_mount: Option<TempDir>,
    names: RefCell<Vec<CgroupName>>,
}

impl Scratch {
    /// `None` if the host cannot provide the requested hierarchy.
    fn new(hierarchy: Hierarchy) -> Option<Self> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = format!(
            "nr-ac-it-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );

        match hierarchy {
            Hierarchy::V2 => {
                let host = CgroupTracker::detect().expect("detecting the host hierarchy");
                if host.hierarchy() != Hierarchy::V2 {
                    eprintln!("skipping: the host does not use the unified hierarchy");
                    return None;
                }
                let root = host.base().join(unique);
                fs::create_dir(&root).expect("creating the scratch cgroup");
                Some(Self::from_parts(root, Hierarchy::V2, None))
            }
            Hierarchy::V1 => {
                let dir = TempDir::new().unwrap();
                let options = format!("none,name={}", unique.replace('-', "_"));
                mount(
                    Some("cgroup"),
                    dir.path(),
                    Some("cgroup"),
                    MsFlags::empty(),
                    Some(options.as_str()),
                )
                .expect("mounting a private v1 hierarchy (needs root and CAP_SYS_ADMIN)");
                Some(Self::from_parts(
                    dir.path().to_path_buf(),
                    Hierarchy::V1,
                    Some(dir),
                ))
            }
        }
    }

    fn from_parts(root: PathBuf, hierarchy: Hierarchy, v1_mount: Option<TempDir>) -> Self {
        Self {
            tracker: CgroupTracker::with_base(&root, hierarchy),
            root,
            v1_mount,
            names: RefCell::default(),
        }
    }

    /// Name of an executable cgroup, registered for cleanup.
    fn name(&self, executable: &str) -> CgroupName {
        let name = CgroupName::new(AGENT, executable);
        self.names.borrow_mut().push(name.clone());
        name
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for name in self.names.borrow().iter() {
            let _ = self.tracker.kill_and_remove(name);
        }
        match &self.v1_mount {
            Some(dir) => {
                let _ = umount(dir.path());
            }
            None => {
                let _ = fs::remove_dir(&self.root);
            }
        }
    }
}

/// Spawns the command and reaps it in the background, as init would once the supervisor is
/// gone. Without reaping, the zombie would keep the cgroup from being removed.
fn spawn_reaped(command: &mut Command) -> u32 {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning the command");
    let pid = child.id();
    thread::spawn(move || {
        let _ = child.wait();
    });
    pid
}

fn sh(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", script]);
    command
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + POLL_TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn members(scratch: &Scratch, name: &CgroupName) -> Vec<i32> {
    scratch.tracker.members(name).unwrap()
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn child_is_in_the_cgroup_when_it_execs_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");

    // `cat` only reports the cgroup it had at `exec`, which is what must already be the final one.
    let mut command = Command::new("cat");
    command.arg("/proc/self/cgroup");
    scratch.tracker.prepare(&name, &mut command).unwrap();
    let output = command.output().unwrap();

    assert!(output.status.success());
    let suffix = format!("/_nr-ac-{AGENT}/exec");
    let cgroups = String::from_utf8(output.stdout).unwrap();
    assert!(
        cgroups.lines().any(|line| line.ends_with(&suffix)),
        "no membership ends with `{suffix}`:\n{cgroups}"
    );
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn child_leads_its_own_process_group_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");
    let mut command = Command::new("sleep");
    command.arg("60");
    scratch.tracker.prepare(&name, &mut command).unwrap();

    let pid = spawn_reaped(&mut command);

    assert_eq!(
        getpgid(Some(Pid::from_raw(pid as i32))).unwrap().as_raw(),
        pid as i32
    );
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn descendants_stay_in_the_cgroup_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");
    let mut command = sh("sleep 60 & sleep 60 & wait");
    scratch.tracker.prepare(&name, &mut command).unwrap();

    let pid = spawn_reaped(&mut command);

    // The shell and its two `sleep` children.
    wait_until("the shell and its children to be members", || {
        members(&scratch, &name).len() == 3
    });
    assert!(members(&scratch, &name).contains(&(pid as i32)));
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn spawn_fails_instead_of_running_outside_the_cgroup_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");
    let marker = scratch.root.join("ran");
    let mut command = sh(&format!("touch {}", marker.display()));
    scratch.tracker.prepare(&name, &mut command).unwrap();
    // The `cgroup.procs` file the child must write to no longer exists.
    fs::remove_dir(scratch.tracker.path_of(&name)).unwrap();

    let result = command.status();

    assert!(result.is_err(), "the command was spawned: {result:?}");
    assert!(!marker.exists(), "the command ran outside of the cgroup");
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn kill_and_remove_stops_the_tree_and_removes_the_cgroup_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");
    let mut command = sh("sleep 60 & sleep 60 & wait");
    scratch.tracker.prepare(&name, &mut command).unwrap();
    spawn_reaped(&mut command);
    wait_until("the tree to be running", || {
        members(&scratch, &name).len() == 3
    });

    scratch.tracker.kill_and_remove(&name).unwrap();

    // Removal only succeeds once the cgroup has no members left.
    assert!(!scratch.tracker.path_of(&name).exists());
    assert!(members(&scratch, &name).is_empty());
}

#[rstest]
#[case::v1(Hierarchy::V1)]
#[case::v2(Hierarchy::V2)]
#[ignore = "needs root and a writable cgroupfs"]
fn new_tracker_finds_the_members_left_by_a_previous_one_as_root(#[case] hierarchy: Hierarchy) {
    let Some(scratch) = Scratch::new(hierarchy) else {
        return;
    };
    let name = scratch.name("exec");
    let mut command = Command::new("sleep");
    command.arg("60");
    scratch.tracker.prepare(&name, &mut command).unwrap();
    let pid = spawn_reaped(&mut command);

    // What a restarted supervisor does: a fresh tracker over the same base.
    let restarted = CgroupTracker::with_base(scratch.tracker.base(), scratch.tracker.hierarchy());

    assert_eq!(restarted.members(&name).unwrap(), vec![pid as i32]);
    restarted.kill_and_remove(&name).unwrap();
    assert!(!restarted.path_of(&name).exists());
}
