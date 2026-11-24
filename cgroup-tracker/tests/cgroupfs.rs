//! Integration tests against the real cgroupfs.
//!
//! They are ignored by default. They need a cgroup v2 host and a cgroup the tests can write to.
//!
//! Only the v1 cases need root: each one mounts a private named v1 hierarchy, and `mount` requires
//! `CAP_SYS_ADMIN`. The v2 cases (`*_v2` and the `detect` test) only need a delegated cgroup.
//!
//! All cases, as root:
//!
//! ```sh
//! CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="sudo systemd-run --scope --quiet --collect -p Delegate=yes --" \
//!   cargo test -p cgroup-tracker --test cgroupfs -- --ignored
//! ```
//!
//! Only the v2 cases, without root. This needs a systemd user session (`/run/user/<uid>`):
//!
//! ```sh
//! CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="systemd-run --user --scope --quiet --collect -p Delegate=yes --" \
//!   cargo test -p cgroup-tracker --test cgroupfs -- --ignored --skip _v1
//! ```
//!
//! Use the `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER` variable on aarch64.
//!
//! The v1 cases also run without root if root mounted the hierarchy beforehand and handed a
//! subtree to the test user. Point `CGROUP_TRACKER_V1_BASE` at it, and the tests neither mount
//! nor unmount:
//!
//! ```sh
//! sudo mkdir -p /mnt/nr-ac-v1
//! sudo mount -t cgroup -o none,name=nr-ac-test cgroup /mnt/nr-ac-v1
//! sudo mkdir /mnt/nr-ac-v1/tests && sudo chown "$USER" /mnt/nr-ac-v1/tests
//! CGROUP_TRACKER_V1_BASE=/mnt/nr-ac-v1/tests \
//!   CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="systemd-run --user --scope --quiet --collect -p Delegate=yes --" \
//!   cargo test -p cgroup-tracker --test cgroupfs -- --ignored
//! sudo umount /mnt/nr-ac-v1
//! ```

#![cfg(target_os = "linux")]

#[path = "cgroupfs/helpers.rs"]
mod helpers;

use cgroup_tracker::{CgroupTracker, Hierarchy};
use helpers::{TestBase, is_alive, stat, wait_until};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use rstest::rstest;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

/// A shell with an orphaned grandchild (the subshell exits before the shell goes on), a child in
/// a new session and a plain background child:
/// - `(sleep 300 &)`: the subshell exits at once, so `sleep` is orphaned (double fork).
/// - `setsid sleep 300 &`: `sleep` runs in a new session (no fork, as the shell has no job control).
/// - `sleep 300 &`: plain background child.
/// - `wait`: keeps the shell alive as a member of the cgroup.
const TREE_SCRIPT: &str = "(sleep 300 &); setsid sleep 300 & sleep 300 & wait";
/// Processes in [TREE_SCRIPT]. Transient states never reach it, as the subshell is gone first.
const TREE_SIZE: usize = 4;
/// A shell with a single background child.
const PAIR_SCRIPT: &str = "sleep 300 & wait";

fn sigkill(pid: i32) {
    kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap();
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn child_execs_inside_cgroup(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("cat");
    let mut command = Command::new("cat");
    command.arg("/proc/self/cgroup").stdout(Stdio::piped());
    let child = base.tracker().cgroup(&name).spawn(command).unwrap();

    let output = child.wait_with_output().unwrap();

    // The child reports its own cgroup, so it was attached before running any of its code.
    assert!(output.status.success());
    let proc_cgroup = String::from_utf8(output.stdout).unwrap();
    assert!(
        base.is_cgroup(&base.tracker().cgroup(&name).path(), &proc_cgroup),
        "child ran in {proc_cgroup}"
    );
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn forks_double_forks_and_setsid_stay_tracked(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("tree");
    let mut child = base.spawn(&name, TREE_SCRIPT);
    let shell = child.id() as i32;
    let test_process = std::process::id() as i32;

    // `setsid` may still be running when the count is reached, so wait for the new session too.
    let mut members = Vec::new();
    wait_until("a member in a new session and an orphaned member", || {
        members = base.tracker().cgroup(&name).members().unwrap();
        let shell_session = stat(shell).map(|s| s.session);
        let stats: Vec<_> = members.iter().filter_map(|pid| stat(*pid)).collect();
        members.len() == TREE_SIZE
            && stats.iter().any(|s| Some(s.session) != shell_session)
            && stats
                .iter()
                .any(|s| s.ppid != shell && s.ppid != test_process)
    });

    assert!(members.contains(&shell));
    for pid in members {
        assert!(base.is_in(pid, &base.tracker().cgroup(&name).path()));
    }

    base.tracker().cgroup(&name).kill_and_remove().unwrap();
    child.wait().unwrap();
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn kill_and_remove_kills_whole_tree(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("tree");
    let mut child = base.spawn(&name, TREE_SCRIPT);
    let members = base.wait_for_members(&name, TREE_SIZE);

    base.tracker().cgroup(&name).kill_and_remove().unwrap();

    assert!(!base.tracker().cgroup(&name).path().exists());
    assert!(base.tracker().cgroup(&name).members().unwrap().is_empty());
    assert_eq!(child.wait().unwrap().signal(), Some(Signal::SIGKILL as i32));
    wait_until("every member to exit", || {
        members.iter().all(|pid| !is_alive(*pid))
    });
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn kill_and_remove_with_unreaped_child(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("pair");
    let mut child = base.spawn(&name, PAIR_SCRIPT);
    let shell = child.id() as i32;
    let members = base.wait_for_members(&name, 2);
    sigkill(shell);
    wait_until("the shell to become a zombie", || {
        stat(shell).is_some_and(|s| s.state == 'Z')
    });

    // The shell is not reaped yet while its child is still running.
    base.tracker().cgroup(&name).kill_and_remove().unwrap();

    assert!(!base.tracker().cgroup(&name).path().exists());
    wait_until("every member to exit", || {
        members.iter().all(|pid| !is_alive(*pid))
    });
    child.wait().unwrap();
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn kill_and_remove_removes_descendant_cgroups(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("pair");
    let mut child = base.spawn(&name, PAIR_SCRIPT);
    let shell = child.id() as i32;
    let members = base.wait_for_members(&name, 2);
    let sleep = *members.iter().find(|pid| **pid != shell).unwrap();
    let nested = base.tracker().cgroup(&name).path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("cgroup.procs"), sleep.to_string()).unwrap();
    assert!(base.is_in(sleep, &nested));
    // Members of descendant cgroups are still reported.
    assert_eq!(base.wait_for_members(&name, 2), members);

    base.tracker().cgroup(&name).kill_and_remove().unwrap();

    assert!(!base.tracker().cgroup(&name).path().exists());
    wait_until("every member to exit", || {
        members.iter().all(|pid| !is_alive(*pid))
    });
    child.wait().unwrap();
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn populated_cgroup_cannot_be_removed(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("pair");
    let mut child = base.spawn(&name, PAIR_SCRIPT);
    base.wait_for_members(&name, 2);

    let err = fs::remove_dir(base.tracker().cgroup(&name).path()).unwrap_err();

    assert_eq!(err.kind(), ErrorKind::ResourceBusy);
    base.tracker().cgroup(&name).kill_and_remove().unwrap();
    child.wait().unwrap();
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn tree_survives_parent_death_and_is_readopted(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("pair");
    let mut child = base.spawn(&name, PAIR_SCRIPT);
    let shell = child.id() as i32;
    let sleep = *base
        .wait_for_members(&name, 2)
        .iter()
        .find(|pid| **pid != shell)
        .unwrap();

    sigkill(shell);
    child.wait().unwrap();

    // The orphan is re-parented but stays in the cgroup.
    assert_eq!(base.wait_for_members(&name, 1), vec![sleep]);
    assert!(is_alive(sleep));
    assert_ne!(stat(sleep).unwrap().ppid, shell);
    assert!(base.is_in(sleep, &base.tracker().cgroup(&name).path()));

    // A new tracker, as after a restart, finds the tree and stops it.
    let restarted = CgroupTracker::with_base(base.tracker().base(), hierarchy);
    assert_eq!(restarted.cgroup(&name).members().unwrap(), vec![sleep]);
    restarted.cgroup(&name).kill_and_remove().unwrap();

    assert!(!restarted.cgroup(&name).path().exists());
    wait_until("the orphan to exit", || !is_alive(sleep));
}

#[rstest]
#[case::v2(Hierarchy::V2)]
#[case::v1(Hierarchy::V1)]
#[ignore = "requires a writable cgroup v2, and root or a premounted hierarchy for v1"]
fn create_on_populated_cgroup_keeps_its_members(#[case] hierarchy: Hierarchy) {
    let base = TestBase::new(hierarchy);
    let name = base.cgroup("pair");
    let mut child = base.spawn(&name, PAIR_SCRIPT);
    let members = base.wait_for_members(&name, 2);

    base.tracker().cgroup(&name).create().unwrap();

    assert_eq!(base.wait_for_members(&name, 2), members);
    for pid in members {
        assert!(is_alive(pid));
        assert!(base.is_in(pid, &base.tracker().cgroup(&name).path()));
    }
    base.tracker().cgroup(&name).kill_and_remove().unwrap();
    child.wait().unwrap();
}

#[test]
#[ignore = "requires a writable cgroup v2"]
fn detect_resolves_own_cgroup_v2() {
    let tracker = CgroupTracker::detect().unwrap();

    assert_eq!(tracker.hierarchy(), Hierarchy::V2);
    let procs = fs::read_to_string(tracker.base().join("cgroup.procs")).unwrap();
    assert!(
        procs.lines().any(|l| l == std::process::id().to_string()),
        "test process not in {}",
        tracker.base().display()
    );
}
