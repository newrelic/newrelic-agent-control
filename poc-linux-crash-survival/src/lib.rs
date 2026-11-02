//! Minimal PoC of crash survival for sub-agents on Linux without elevation.
//!
//! With `KillMode=process` (or when AC is simply `kill -9`'d outside systemd) the sub-agents keep
//! running after Agent Control dies. What is hard is finding them again. This crate holds the
//! building blocks for the four ways the examples try:
//!
//! - bookkeeping: [`AdoptedProcess`] opened from a stored `(pid, start_time)`,
//! - environment tag: [`tag`] scans `/proc` for the tag and picks the group leader,
//! - per-sub-agent cgroup: [`cgroup`] keeps the members in a named cgroup,
//! - shim: [`shim`] is a tiny parent process AC talks to over a unix socket.
#![cfg(target_os = "linux")]

pub mod cgroup;
pub mod shim;
pub mod tag;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Interval used by every polling loop in the PoC.
const POLL: Duration = Duration::from_millis(50);

/// The fields of `/proc/<pid>/stat` the PoC needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcStat {
    /// Process state (`R`, `S`, `Z`, ...).
    pub state: char,
    pub ppid: u32,
    pub pgid: u32,
    /// `starttime`: clock ticks since boot. Stable for the process lifetime, so it tells the
    /// recorded process apart from a later one that reuses the pid.
    pub start_time: u64,
}

/// Reads `/proc/<pid>/stat`; `None` if the process does not exist.
pub fn read_stat(pid: u32) -> Option<ProcStat> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Parses `/proc/<pid>/stat`. `comm` (field 2) is parenthesized and may contain spaces or parens,
/// so the numeric fields are located after the *last* `)`.
pub fn parse_stat(stat: &str) -> Option<ProcStat> {
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // `state` (field 3) is index 0: ppid is field 4, pgrp field 5, starttime field 22.
    Some(ProcStat {
        state: fields.first()?.chars().next()?,
        ppid: fields.get(1)?.parse().ok()?,
        pgid: fields.get(2)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

/// Whether `pid` is a live process started at `start_time`. A zombie counts as gone: nobody has
/// reaped it yet (inside a container without an init, nobody ever will) but it does not run.
pub fn is_alive(pid: u32, start_time: u64) -> bool {
    read_stat(pid).is_some_and(|s| s.start_time == start_time && !matches!(s.state, 'Z' | 'X'))
}

/// A process this instance did not spawn, re-opened after a crash. We are not its parent, so there
/// is no `waitpid` and no exit status: liveness is a poll of `/proc`.
#[derive(Debug, Clone, Copy)]
pub struct AdoptedProcess {
    pub pid: u32,
    pub start_time: u64,
}

impl AdoptedProcess {
    /// Adopts `pid` only if it is alive and started at `start_time` (guards against pid reuse).
    pub fn adopt(pid: u32, start_time: u64) -> Option<Self> {
        is_alive(pid, start_time).then_some(Self { pid, start_time })
    }

    pub fn is_alive(&self) -> bool {
        is_alive(self.pid, self.start_time)
    }

    /// Waits up to `timeout` for the process to exit. Returns `true` if it did.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !self.is_alive() {
                return true;
            }
            std::thread::sleep(POLL);
        }
        !self.is_alive()
    }

    pub fn kill(&self) -> io::Result<()> {
        kill(Pid::from_raw(self.pid as i32), Signal::SIGKILL).map_err(io::Error::from)
    }
}

/// Spawns `cmd` the way AC would: as the leader of its own process group, with no stdio attached
/// (a pipe to AC would break when AC dies).
pub fn spawn_detached(cmd: &mut Command) -> io::Result<Child> {
    cmd.process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Whether the effective uid is root.
pub fn is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

/// Polls `check` every [`POLL`] until it returns `Some` or `timeout` elapses.
pub fn wait_until<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_handles_parens_and_spaces_in_comm() {
        let mut fields = vec!["0".to_string(); 30];
        fields[0] = "S".into();
        fields[1] = "42".into();
        fields[2] = "41".into();
        fields[19] = "987".into();
        let stat = format!("1 (we) (ird name) {}", fields.join(" "));

        assert_eq!(
            parse_stat(&stat),
            Some(ProcStat {
                state: 'S',
                ppid: 42,
                pgid: 41,
                start_time: 987
            })
        );
    }

    #[test]
    fn adopt_rejects_a_mismatched_start_time_and_accepts_the_real_one() {
        let mut child = spawn_detached(Command::new("sleep").arg("30")).unwrap();
        let start = read_stat(child.id()).unwrap().start_time;

        assert!(AdoptedProcess::adopt(child.id(), start + 1).is_none());
        let adopted = AdoptedProcess::adopt(child.id(), start).unwrap();
        adopted.kill().unwrap();
        child.wait().unwrap();
        assert!(!adopted.is_alive());
    }
}
