//! Backend A: plain cgroupfs, with the child moving itself into the cgroup between `fork` and
//! `exec`, so no instruction of the target program runs outside of it.
//!
//! Systemd tracks units in the unified hierarchy on v2 and on hybrid hosts (where the other
//! controllers stay on v1), and in the `name=systemd` hierarchy on pure v1. The cgroup has to be
//! created in that hierarchy for the process to leave the cgroup of the unit that started it.
use crate::error::{Error, Result, ctx};
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const ROOT: &str = "/sys/fs/cgroup";
const CGROUP2_SUPER_MAGIC: i64 = 0x6367_7270;
const PARENT: &str = "ac-poc";
const PROCS: &str = "cgroup.procs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    V2,
    Hybrid,
    V1,
}

fn is_cgroup2(path: &str) -> bool {
    let Ok(path) = CString::new(path) else {
        return false;
    };
    // SAFETY: `statfs` only writes to the zeroed struct we own.
    unsafe {
        let mut st: libc::statfs = std::mem::zeroed();
        libc::statfs(path.as_ptr(), &mut st) == 0 && st.f_type as i64 == CGROUP2_SUPER_MAGIC
    }
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

    /// Makes the command move itself into this cgroup before `exec`. Everything done in the
    /// child is async-signal-safe: `open`, `write` and `close` on a path converted beforehand.
    pub fn attach_on_spawn(&self, command: &mut Command) -> Result<()> {
        let procs = CString::new(self.dir.join(PROCS).into_os_string().into_encoded_bytes())
            .map_err(ctx("building cgroup.procs path"))?;
        // SAFETY: the closure only calls async-signal-safe functions and does not allocate.
        unsafe {
            command.pre_exec(move || {
                let fd = libc::open(procs.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                let written = libc::write(fd, b"0".as_ptr().cast(), 1);
                let err = io::Error::last_os_error();
                libc::close(fd);
                if written == 1 { Ok(()) } else { Err(err) }
            });
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
                // SAFETY: plain `kill(2)` call.
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(Error::new("cgroup still has processes after SIGKILL"))
    }
}
