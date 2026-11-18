//! Backend A: plain cgroupfs, with the child moving itself into the cgroup between `fork` and
//! `exec`, so no instruction of the target program runs outside of it.
//!
//! Systemd tracks units in the unified hierarchy on v2 and on hybrid hosts (where the other
//! controllers stay on v1), and in the `name=systemd` hierarchy on pure v1. The cgroup has to be
//! created in that hierarchy for the process to leave the cgroup of the unit that started it.
use crate::error::{Error, Result, ctx};
use nix::sys::signal::{Signal, kill};
use nix::unistd::{Pid, write};
use std::fs::{self, File};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const ROOT: &str = "/sys/fs/cgroup";
const PARENT: &str = "ac-poc";
const PROCS: &str = "cgroup.procs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    V2,
    Hybrid,
    V1,
}

/// Only the root of a cgroup2 mount has `cgroup.controllers`.
fn is_cgroup2(path: &str) -> bool {
    Path::new(path).join("cgroup.controllers").exists()
}

pub fn detect() -> Layout {
    if is_cgroup2(ROOT) {
        Layout::V2
    } else if is_cgroup2(&format!("{ROOT}/unified")) {
        Layout::Hybrid
    } else {
        Layout::V1
    }
}

pub struct Cgroup {
    dir: PathBuf,
}

impl Cgroup {
    pub fn new(name: &str) -> Self {
        let base = match detect() {
            Layout::V2 => Path::new(ROOT).to_path_buf(),
            Layout::Hybrid => Path::new(ROOT).join("unified"),
            Layout::V1 => Path::new(ROOT).join("systemd"),
        };
        Self {
            dir: base.join(PARENT).join(name),
        }
    }

    pub fn create(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(ctx("creating cgroup"))
    }

    /// Makes the command move itself into this cgroup before `exec`. `cgroup.procs` is opened here,
    /// so the child only performs one `write`, which is async-signal-safe and does not allocate.
    pub fn attach_on_spawn(&self, command: &mut Command) -> Result<()> {
        let procs = File::options()
            .write(true)
            .open(self.dir.join(PROCS))
            .map_err(ctx("opening cgroup.procs"))?;
        // SAFETY: the closure only calls `write(2)` and does not allocate.
        unsafe {
            command.pre_exec(move || write(&procs, b"0").map(drop).map_err(io::Error::from));
        }
        Ok(())
    }

    pub fn pids(&self) -> Vec<u32> {
        fs::read_to_string(self.dir.join(PROCS))
            .map(|s| s.lines().filter_map(|l| l.trim().parse().ok()).collect())
            .unwrap_or_default()
    }

    /// Kills every process of the cgroup, waits for it to empty and removes it.
    pub fn kill_and_remove(&self) -> Result<()> {
        for _ in 0..50 {
            let pids = self.pids();
            if pids.is_empty() {
                return match fs::remove_dir(&self.dir) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(Error::new(format!("removing cgroup: {e}"))),
                };
            }
            for pid in pids {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(Error::new("cgroup still has processes after SIGKILL"))
    }
}
