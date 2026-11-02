//! Per-sub-agent cgroup: the cgroup directory is the persistent, named record. Membership is
//! maintained by the kernel, descendants included, and survives whatever the sub-agent does to its
//! own environment, argv or uid.
//!
//! Needs cgroup v2 and write access to the parent cgroup (root, or a delegated subtree).

use nix::fcntl::{OFlag, open};
use nix::sys::signal::{Signal, kill};
use nix::sys::stat::Mode;
use nix::unistd::{Pid, write};
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

const CGROUP_MOUNT: &str = "/sys/fs/cgroup";

/// The cgroup AC itself runs in (cgroup v2 `0::<path>` entry of `/proc/self/cgroup`).
pub fn own_cgroup_dir() -> io::Result<PathBuf> {
    let self_cgroup = std::fs::read_to_string("/proc/self/cgroup")?;
    let relative = self_cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| io::Error::other("no cgroup v2 unified hierarchy (no `0::` entry)"))?;
    Ok(Path::new(CGROUP_MOUNT).join(relative.trim().trim_start_matches('/')))
}

/// A named cgroup directory.
#[derive(Debug, Clone)]
pub struct Cgroup {
    path: PathBuf,
}

impl Cgroup {
    /// Creates `<parent>/<name>`, or reuses it if it already exists (get-or-create tolerates
    /// leftovers from a crash).
    pub fn create(parent: &Path, name: &str) -> io::Result<Self> {
        let path = parent.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
        Ok(Self { path })
    }

    /// Looks `<parent>/<name>` up by name, as the restarted AC does after a crash.
    pub fn open(parent: &Path, name: &str) -> Option<Self> {
        let path = parent.join(name);
        path.is_dir().then_some(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every pid in the cgroup, zombies included.
    pub fn procs(&self) -> Vec<u32> {
        std::fs::read_to_string(self.path.join("cgroup.procs"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }

    /// Members that are actually running. A zombie stays in `cgroup.procs` until its parent reaps
    /// it, which inside a container without an init never happens.
    pub fn live_members(&self) -> Vec<u32> {
        self.procs()
            .into_iter()
            .filter(|&pid| crate::read_stat(pid).is_some_and(|s| !matches!(s.state, 'Z' | 'X')))
            .collect()
    }

    /// Kills every member. Uses `cgroup.kill` (Linux 5.14+), which is atomic against forks; on older
    /// kernels falls back to a loop over `cgroup.procs`, which can miss a process forking meanwhile.
    pub fn kill_all(&self) -> io::Result<()> {
        let kill_file = self.path.join("cgroup.kill");
        if kill_file.exists() {
            return std::fs::write(kill_file, "1");
        }
        for pid in self.procs() {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
        Ok(())
    }

    /// Removes the (empty) cgroup, retrying while the kernel finishes tearing members down.
    pub fn remove(&self, timeout: Duration) -> io::Result<()> {
        let mut last = Ok(());
        crate::wait_until(timeout, || {
            last = std::fs::remove_dir(&self.path);
            last.is_ok().then_some(())
        });
        last
    }
}

/// Spawns `cmd` inside `cgroup`. The child writes `0` (itself) to `cgroup.procs` between `fork` and
/// `exec`, so there is no window where it runs outside the cgroup. Only syscalls, no allocation.
pub fn spawn_in_cgroup(cmd: &mut Command, cgroup: &Cgroup) -> io::Result<Child> {
    let procs = CString::new(cgroup.path.join("cgroup.procs").as_os_str().as_bytes())
        .map_err(io::Error::other)?;
    // SAFETY: `pre_exec` runs in the forked child, before `exec`, where only async-signal-safe
    // calls are allowed (the parent may be multithreaded, so e.g. taking the allocator lock could
    // deadlock). The closure only calls `open` and `write`, and the path was converted to a
    // `CString` beforehand, so it does not allocate.
    unsafe {
        cmd.pre_exec(move || {
            // Open `cgroup.procs` of the target cgroup.
            let fd = open(procs.as_c_str(), OFlag::O_WRONLY, Mode::empty())?;
            // Writing `0` moves the calling process (here, the child itself) into that cgroup.
            // Doing it before `exec` means the sub-agent never runs a single instruction outside
            // its cgroup, so there is no spawn-to-move window for a crash to leave an untracked
            // orphan. The kernel then keeps it, and all its descendants, in the cgroup.
            write(&fd, b"0")?;
            // `fd` is closed on drop; any error above makes the spawn fail instead of running the
            // sub-agent outside the cgroup.
            Ok(())
        });
    }
    crate::spawn_detached(cmd)
}
