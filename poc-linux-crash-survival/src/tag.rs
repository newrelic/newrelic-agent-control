//! Environment variable tag discovery: every sub-agent is spawned with markers in its environment
//! and, after a crash, the restarted AC scans `/proc` for them.

use crate::{ProcStat, read_stat};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

pub const ENV_AGENT_ID: &str = "NR_AC_AGENT_ID";
pub const ENV_EXEC_ID: &str = "NR_AC_EXEC_ID";
/// Identifies the AC instance (its data dir), so one AC never adopts or kills another's processes.
pub const ENV_INSTANCE: &str = "NR_AC_INSTANCE";

/// What a surviving process must carry to be considered ours.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    pub agent_id: &'a str,
    pub exec_id: &'a str,
    pub instance: &'a str,
    /// Program followed by args, exactly as spawned.
    pub cmdline: &'a [OsString],
}

impl Identity<'_> {
    /// Adds the markers to `cmd`. Call it after setting the executable's own env.
    pub fn tag(&self, cmd: &mut Command) {
        cmd.env(ENV_AGENT_ID, self.agent_id)
            .env(ENV_EXEC_ID, self.exec_id)
            .env(ENV_INSTANCE, self.instance);
    }
}

/// A live process that matches an [`Identity`].
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub start_time: u64,
}

/// Every same-uid process in `/proc` that matches `identity`. There is no cgroup scope, so it
/// works the same on cgroup v1, v2 and hosts without systemd.
pub fn scan(identity: &Identity) -> Vec<Candidate> {
    scan_in(
        Path::new("/proc"),
        nix::unistd::geteuid().as_raw(),
        identity,
    )
}

fn scan_in(proc_root: &Path, uid: u32, identity: &Identity) -> Vec<Candidate> {
    std::fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| read_candidate(proc_root, pid, uid, identity))
        .collect()
}

fn read_candidate(proc_root: &Path, pid: u32, uid: u32, identity: &Identity) -> Option<Candidate> {
    let dir = proc_root.join(pid.to_string());
    if std::fs::metadata(&dir).ok()?.uid() != uid {
        return None;
    }
    // cmdline first: the cheapest reject when scanning every process.
    if !cmdline_matches(&std::fs::read(dir.join("cmdline")).ok()?, identity.cmdline) {
        return None;
    }
    if !has_markers(&std::fs::read(dir.join("environ")).ok()?, identity) {
        return None;
    }
    let ProcStat {
        state,
        ppid,
        pgid,
        start_time,
    } = read_stat(pid)?;
    if matches!(state, 'Z' | 'X') {
        return None;
    }
    Some(Candidate {
        pid,
        ppid,
        pgid,
        start_time,
    })
}

fn has_markers(environ: &[u8], identity: &Identity) -> bool {
    let expected = [
        format!("{ENV_AGENT_ID}={}", identity.agent_id),
        format!("{ENV_EXEC_ID}={}", identity.exec_id),
        format!("{ENV_INSTANCE}={}", identity.instance),
    ];
    let mut found = [false; 3];
    for entry in environ.split(|b| *b == 0) {
        for (found, want) in found.iter_mut().zip(&expected) {
            *found |= entry == want.as_bytes();
        }
    }
    found.iter().all(|f| *f)
}

fn cmdline_matches(cmdline: &[u8], expected: &[OsString]) -> bool {
    let cmdline = cmdline.strip_suffix(&[0]).unwrap_or(cmdline);
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(arg, want)| OsStr::from_bytes(arg) == want.as_os_str())
}

/// Picks the top-level process: the one leading its own process group (`pgid == pid`, which is how
/// AC spawns sub-agents). Forked workers share the group but not the leader's pid, so they are
/// never picked; if the leader is gone nothing is adopted. A leader whose parent is also a
/// candidate is skipped (a worker that called `setsid`); the oldest of what remains wins.
pub fn select_root(candidates: &[Candidate]) -> Option<&Candidate> {
    let pids: HashSet<u32> = candidates.iter().map(|c| c.pid).collect();
    candidates
        .iter()
        .filter(|c| c.pgid == c.pid && !pids.contains(&c.ppid))
        .min_by_key(|c| c.start_time)
}

/// SIGKILLs the process group of every orphan (workers whose leader is gone), so they do not keep
/// running unsupervised next to a respawned copy. The group of AC itself and init's are never
/// signaled; those orphans are killed one by one.
pub fn kill_orphans(orphans: &[Candidate]) -> io::Result<()> {
    use nix::sys::signal::kill;
    let own_pgid = nix::unistd::getpgrp().as_raw() as u32;
    let mut groups = HashSet::new();
    for orphan in orphans {
        let result = if orphan.pgid > 1 && orphan.pgid != own_pgid {
            if !groups.insert(orphan.pgid) {
                continue;
            }
            killpg(Pid::from_raw(orphan.pgid as i32), Signal::SIGKILL)
        } else {
            kill(Pid::from_raw(orphan.pid as i32), Signal::SIGKILL)
        };
        match result {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(pid: u32, ppid: u32, pgid: u32, start_time: u64) -> Candidate {
        Candidate {
            pid,
            ppid,
            pgid,
            start_time,
        }
    }

    #[test]
    fn root_is_the_group_leader_even_if_a_worker_is_older() {
        let found = [candidate(20, 10, 10, 100), candidate(10, 1, 10, 500)];
        assert_eq!(select_root(&found).map(|c| c.pid), Some(10));
    }

    #[test]
    fn no_root_when_only_orphaned_workers_survive() {
        let found = [candidate(20, 1, 10, 100), candidate(21, 1, 10, 101)];
        assert_eq!(select_root(&found), None);
    }

    #[test]
    fn a_setsid_worker_whose_parent_matches_is_skipped() {
        let found = [candidate(10, 1, 10, 500), candidate(20, 10, 20, 100)];
        assert_eq!(select_root(&found).map(|c| c.pid), Some(10));
    }

    #[test]
    fn marker_values_must_match_exactly() {
        let cmdline = [];
        let id = Identity {
            agent_id: "a",
            exec_id: "e",
            instance: "/i",
            cmdline: &cmdline,
        };
        let env = |parts: &[&str]| -> Vec<u8> {
            parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
        };
        assert!(has_markers(
            &env(&["NR_AC_AGENT_ID=a", "NR_AC_EXEC_ID=e", "NR_AC_INSTANCE=/i"]),
            &id
        ));
        assert!(!has_markers(
            &env(&["NR_AC_AGENT_ID=ab", "NR_AC_EXEC_ID=e", "NR_AC_INSTANCE=/i"]),
            &id
        ));
        assert!(!has_markers(
            &env(&["NR_AC_AGENT_ID=a", "NR_AC_EXEC_ID=e"]),
            &id
        ));
    }
}
