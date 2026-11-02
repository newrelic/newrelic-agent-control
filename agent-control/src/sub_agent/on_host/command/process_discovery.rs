//! PoC, Linux only: finds a still-running process a previous Agent Control instance spawned for
//! an `(agent_id, exec_id)` pair, without any persisted bookkeeping.
//!
//! Every spawned executable is tagged with [`ENV_AGENT_ID`], [`ENV_EXEC_ID`] and
//! [`ENV_INSTANCE`]. After a crash (`KillMode=process` leaves the sub-agents running), the
//! restarted instance scans every process on the host for the ones carrying all three markers
//! and the expected command line, and adopts the one that leads its own process group (see
//! [`select_root`]). The instance marker is what keeps one Agent Control from adopting, or
//! killing, another one's processes.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Env var carrying the owning agent id; set on every executable Agent Control spawns.
pub const ENV_AGENT_ID: &str = "NR_AC_AGENT_ID";
/// Env var carrying the owning executable id; set on every executable Agent Control spawns.
pub const ENV_EXEC_ID: &str = "NR_AC_EXEC_ID";
/// Env var identifying the Agent Control instance that spawned the executable (its data
/// directory); set on every executable Agent Control spawns.
pub const ENV_INSTANCE: &str = "NR_AC_INSTANCE";

const PROC_ROOT: &str = "/proc";

/// A live process that looks like the executable Agent Control spawned for an `(agent, exec)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub pid: u32,
    pub ppid: u32,
    /// Process group id; Agent Control spawns every executable as its own group leader, so the
    /// top-level process has `pgid == pid`.
    pub pgid: u32,
    /// `starttime` from `/proc/<pid>/stat`, used to detect pid reuse after discovery.
    pub start_time_marker: u64,
}

/// What a surviving process must carry to be considered ours.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    pub agent_id: &'a str,
    pub exec_id: &'a str,
    pub instance: &'a str,
    /// Program followed by args, exactly as spawned.
    pub cmdline: &'a [OsString],
}

/// Result of scanning for survivors of a previous Agent Control instance.
#[derive(Debug, Default, PartialEq)]
pub struct Discovery {
    /// The top-level process to adopt, if its group leader survived.
    pub root: Option<Candidate>,
    /// Matching processes left over without their group leader (a crashed master's workers).
    /// Empty whenever `root` is set. They must be killed before respawning, or they would run
    /// unsupervised next to the new copy.
    pub orphans: Vec<Candidate>,
}

/// Looks for processes spawned for `identity` among every process on the host owned by the same
/// user.
pub fn discover(identity: &Identity) -> Discovery {
    let pids = all_pids(Path::new(PROC_ROOT));
    discover_in(
        Path::new(PROC_ROOT),
        &pids,
        nix::unistd::geteuid().as_raw(),
        identity,
    )
}

/// Every numeric entry of `proc_root`.
fn all_pids(proc_root: &Path) -> Vec<u32> {
    std::fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect()
}

/// [`discover`] against an arbitrary `/proc` root and candidate pid list, so it can be tested
/// against a fake `/proc` tree.
fn discover_in(proc_root: &Path, pids: &[u32], uid: u32, identity: &Identity) -> Discovery {
    let candidates: Vec<Candidate> = pids
        .iter()
        .filter_map(|&pid| read_candidate(proc_root, pid, uid, identity))
        .collect();
    match select_root(&candidates).cloned() {
        Some(root) => Discovery {
            root: Some(root),
            ..Discovery::default()
        },
        None => Discovery {
            orphans: candidates,
            ..Discovery::default()
        },
    }
}

/// How an orphan is killed.
#[derive(Debug, Clone, Copy, PartialEq)]
enum KillTarget {
    /// The whole process group, which also takes down any descendant that no longer matches.
    Group(u32),
    /// A single process.
    Pid(u32),
}

/// Decides what to signal for `orphans`. A group is only killed when it is not Agent Control's
/// own (processes spawned before groups were used share it), and is a real group (`> 1`);
/// otherwise just the matched pids are killed.
fn kill_targets(orphans: &[Candidate], own_pgid: u32) -> Vec<KillTarget> {
    let mut targets = Vec::new();
    for orphan in orphans {
        let target = if orphan.pgid > 1 && orphan.pgid != own_pgid {
            KillTarget::Group(orphan.pgid)
        } else {
            KillTarget::Pid(orphan.pid)
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

/// SIGKILLs the survivors in [`Discovery::orphans`]. Returns one error per target that could
/// not be signaled; a target that already exited (`ESRCH`) is not an error.
pub fn kill_orphans(orphans: &[Candidate]) -> Vec<std::io::Error> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill, killpg};
    use nix::unistd::{Pid, getpgrp};

    kill_targets(orphans, getpgrp().as_raw() as u32)
        .into_iter()
        .filter_map(|target| {
            let result = match target {
                KillTarget::Group(pgid) => killpg(Pid::from_raw(pgid as i32), Signal::SIGKILL),
                KillTarget::Pid(pid) => kill(Pid::from_raw(pid as i32), Signal::SIGKILL),
            };
            match result {
                Ok(()) | Err(Errno::ESRCH) => None,
                Err(err) => Some(std::io::Error::from(err)),
            }
        })
        .collect()
}

/// Picks the top-level process among `candidates`: the one leading its own process group
/// (`pgid == pid`), which is how Agent Control spawns every executable. Forked workers share the
/// leader's group but not its pid, so they are never picked, and if the leader is gone no
/// survivor is adopted (they are reported as orphans to kill before respawning). Among leaders, one whose parent is itself a
/// candidate is skipped (a worker that called `setsid`); if several remain, the oldest wins.
fn select_root(candidates: &[Candidate]) -> Option<&Candidate> {
    let pids: HashSet<u32> = candidates.iter().map(|c| c.pid).collect();
    candidates
        .iter()
        .filter(|c| c.pgid == c.pid && !pids.contains(&c.ppid))
        .min_by_key(|c| c.start_time_marker)
}

fn read_candidate(proc_root: &Path, pid: u32, uid: u32, identity: &Identity) -> Option<Candidate> {
    let dir = proc_root.join(pid.to_string());

    if std::fs::metadata(&dir).ok()?.uid() != uid {
        return None;
    }
    // cmdline first: it is the cheapest reject when scanning every process on the host.
    if !cmdline_matches(&std::fs::read(dir.join("cmdline")).ok()?, identity.cmdline) {
        return None;
    }
    if !has_markers(&std::fs::read(dir.join("environ")).ok()?, identity) {
        return None;
    }
    let (ppid, pgid, start_time_marker) =
        parse_stat(&std::fs::read_to_string(dir.join("stat")).ok()?)?;
    Some(Candidate {
        pid,
        ppid,
        pgid,
        start_time_marker,
    })
}

/// Whether the NUL-separated `environ` carries all markers with exactly these values.
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

/// Whether the NUL-separated `cmdline` is exactly `expected`.
fn cmdline_matches(cmdline: &[u8], expected: &[OsString]) -> bool {
    let cmdline = cmdline.strip_suffix(&[0]).unwrap_or(cmdline);
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(arg, want)| OsStr::from_bytes(arg) == want.as_os_str())
}

/// Extracts `(ppid, pgrp, starttime)` from the contents of `/proc/<pid>/stat`.
fn parse_stat(stat: &str) -> Option<(u32, u32, u64)> {
    // `comm` (field 2) is parenthesized and may itself contain spaces or parens, so split on
    // the *last* ')' rather than whitespace to find where the numeric fields resume.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // `state` (field 3) is index 0, so `ppid` (field 4) is index 1, `pgrp` (field 5) is index 2
    // and `starttime` (field 22) is index 19.
    Some((
        fields.get(1)?.parse().ok()?,
        fields.get(2)?.parse().ok()?,
        fields.get(19)?.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const AGENT: &str = "agent-a";
    const EXEC: &str = "exec-a";
    const INSTANCE: &str = "/var/lib/ac-a";

    fn cmdline(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn stat_line(comm: &str, ppid: u32, pgid: u32, starttime: u64) -> String {
        // Fields after `comm`: state(3) ppid(4) pgrp(5) ... starttime(22).
        let mut fields = vec!["0".to_string(); 30];
        fields[0] = "S".to_string();
        fields[1] = ppid.to_string();
        fields[2] = pgid.to_string();
        fields[19] = starttime.to_string();
        format!("1 ({comm}) {}", fields.join(" "))
    }

    struct FakeProc {
        dir: tempfile::TempDir,
    }

    impl FakeProc {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        /// Adds a process leading its own group, as Agent Control spawns them.
        fn add(&self, pid: u32, env: &[&str], argv: &[&str], ppid: u32, starttime: u64) {
            self.add_in_group(pid, pid, env, argv, ppid, starttime);
        }

        fn add_in_group(
            &self,
            pid: u32,
            pgid: u32,
            env: &[&str],
            argv: &[&str],
            ppid: u32,
            starttime: u64,
        ) {
            let dir = self.dir.path().join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("environ"), nul_joined(env)).unwrap();
            std::fs::write(dir.join("cmdline"), nul_joined(argv)).unwrap();
            std::fs::write(dir.join("stat"), stat_line("sleep", ppid, pgid, starttime)).unwrap();
        }

        fn scan(&self, pids: &[u32], uid: u32, expected: &[&str]) -> Discovery {
            let cmdline = cmdline(expected);
            let identity = Identity {
                agent_id: AGENT,
                exec_id: EXEC,
                instance: INSTANCE,
                cmdline: &cmdline,
            };
            discover_in(self.dir.path(), pids, uid, &identity)
        }

        fn discover(&self, pids: &[u32], uid: u32, expected: &[&str]) -> Option<Candidate> {
            self.scan(pids, uid, expected).root
        }
    }

    fn nul_joined(parts: &[&str]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
    }

    fn own_uid() -> u32 {
        nix::unistd::geteuid().as_raw()
    }

    fn markers() -> [&'static str; 3] {
        [
            "NR_AC_AGENT_ID=agent-a",
            "NR_AC_EXEC_ID=exec-a",
            "NR_AC_INSTANCE=/var/lib/ac-a",
        ]
    }

    #[test]
    fn parse_stat_handles_parens_and_spaces_in_comm() {
        let stat = stat_line("we) (ird name", 42, 41, 987);
        assert_eq!(parse_stat(&stat), Some((42, 41, 987)));
    }

    #[test]
    fn all_pids_lists_only_numeric_entries() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/sleep"], 1, 1);
        std::fs::create_dir(proc.dir.path().join("self")).unwrap();
        std::fs::write(proc.dir.path().join("uptime"), "1").unwrap();

        assert_eq!(all_pids(proc.dir.path()), vec![10]);
    }

    #[test]
    fn ignores_a_process_from_another_agent_control_instance() {
        let proc = FakeProc::new();
        proc.add(
            10,
            &[
                "NR_AC_AGENT_ID=agent-a",
                "NR_AC_EXEC_ID=exec-a",
                "NR_AC_INSTANCE=/var/lib/ac-b",
            ],
            &["/bin/sleep", "30"],
            1,
            500,
        );

        assert_eq!(
            proc.scan(&[10], own_uid(), &["/bin/sleep", "30"]),
            Discovery::default()
        );
    }

    #[test]
    fn finds_a_process_with_matching_markers_and_cmdline() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);

        let found = proc.discover(&[10], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(
            found,
            Some(Candidate {
                pid: 10,
                ppid: 1,
                pgid: 10,
                start_time_marker: 500
            })
        );
    }

    #[test]
    fn ignores_processes_for_another_agent_or_exec() {
        let proc = FakeProc::new();
        proc.add(
            10,
            &[
                "NR_AC_AGENT_ID=other",
                "NR_AC_EXEC_ID=exec-a",
                "NR_AC_INSTANCE=/var/lib/ac-a",
            ],
            &["/bin/sleep", "30"],
            1,
            500,
        );
        proc.add(
            11,
            &[
                "NR_AC_AGENT_ID=agent-a",
                "NR_AC_EXEC_ID=other",
                "NR_AC_INSTANCE=/var/lib/ac-a",
            ],
            &["/bin/sleep", "30"],
            1,
            501,
        );
        proc.add(12, &[], &["/bin/sleep", "30"], 1, 502);

        assert_eq!(
            proc.discover(&[10, 11, 12], own_uid(), &["/bin/sleep", "30"]),
            None
        );
    }

    #[test]
    fn ignores_a_process_with_a_different_cmdline() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/fluent-bit"], 1, 500);

        assert_eq!(proc.discover(&[10], own_uid(), &["/bin/sleep", "30"]), None);
    }

    #[test]
    fn ignores_a_process_owned_by_another_user() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);

        assert_eq!(
            proc.discover(&[10], own_uid() + 1, &["/bin/sleep", "30"]),
            None
        );
    }

    #[test]
    fn ignores_pids_that_vanished() {
        let proc = FakeProc::new();

        assert_eq!(proc.discover(&[99], own_uid(), &["/bin/sleep", "30"]), None);
    }

    #[test]
    fn prefers_the_group_leader_over_a_forked_worker_with_the_same_markers() {
        let proc = FakeProc::new();
        // The worker is older by start time, to prove leadership decides, not age.
        proc.add_in_group(20, 10, &markers(), &["/bin/sleep", "30"], 10, 100);
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);

        let found = proc.discover(&[20, 10], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(found.map(|c| c.pid), Some(10));
    }

    #[test]
    fn reports_orphaned_workers_when_their_leader_is_gone() {
        let proc = FakeProc::new();
        // The leader (pid 10) is gone; its workers were reparented to init.
        proc.add_in_group(20, 10, &markers(), &["/bin/sleep", "30"], 1, 100);
        proc.add_in_group(21, 10, &markers(), &["/bin/sleep", "30"], 1, 101);

        let scan = proc.scan(&[20, 21], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(scan.root, None);
        assert_eq!(
            scan.orphans.iter().map(|c| c.pid).collect::<Vec<_>>(),
            vec![20, 21]
        );
    }

    #[test]
    fn reports_no_orphans_when_the_leader_survived() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);
        proc.add_in_group(20, 10, &markers(), &["/bin/sleep", "30"], 10, 501);

        let scan = proc.scan(&[10, 20], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(scan.root.map(|c| c.pid), Some(10));
        assert!(scan.orphans.is_empty());
    }

    fn orphan(pid: u32, pgid: u32) -> Candidate {
        Candidate {
            pid,
            ppid: 1,
            pgid,
            start_time_marker: 0,
        }
    }

    #[test]
    fn kills_each_orphaned_group_once() {
        let orphans = [orphan(20, 10), orphan(21, 10), orphan(31, 30)];

        assert_eq!(
            kill_targets(&orphans, 999),
            vec![KillTarget::Group(10), KillTarget::Group(30)]
        );
    }

    #[test]
    fn never_kills_agent_controls_own_group_or_init() {
        // Processes started before groups were used share AC's own group; pgid 1 is init's.
        let orphans = [orphan(20, 999), orphan(21, 1)];

        assert_eq!(
            kill_targets(&orphans, 999),
            vec![KillTarget::Pid(20), KillTarget::Pid(21)]
        );
    }

    #[test]
    fn kill_orphans_kills_a_real_orphaned_group_and_ignores_gone_ones() {
        use std::os::unix::process::CommandExt;
        let mut child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();

        let errors = kill_orphans(&[orphan(pid, pid), orphan(4_194_000, 4_194_000)]);

        assert!(errors.is_empty(), "{errors:?}");
        assert!(!child.wait().unwrap().success());
    }

    #[test]
    fn skips_a_leader_whose_parent_is_also_a_match() {
        let proc = FakeProc::new();
        // A worker that called `setsid` leads its own group but is still the master's child.
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);
        proc.add(20, &markers(), &["/bin/sleep", "30"], 10, 100);

        let found = proc.discover(&[20, 10], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(found.map(|c| c.pid), Some(10));
    }

    #[test]
    fn picks_the_oldest_when_several_top_level_processes_match() {
        let proc = FakeProc::new();
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 700);
        proc.add(11, &markers(), &["/bin/sleep", "30"], 1, 300);

        let found = proc.discover(&[10, 11], own_uid(), &["/bin/sleep", "30"]);

        assert_eq!(found.map(|c| c.pid), Some(11));
    }

    #[test]
    fn marker_values_must_match_exactly() {
        let cmdline = [];
        let identity = Identity {
            agent_id: AGENT,
            exec_id: EXEC,
            instance: INSTANCE,
            cmdline: &cmdline,
        };
        assert!(!has_markers(
            &nul_joined(&[
                "NR_AC_AGENT_ID=agent-ab",
                "NR_AC_EXEC_ID=exec-a",
                "NR_AC_INSTANCE=/var/lib/ac-a"
            ]),
            &identity
        ));
        assert!(has_markers(&nul_joined(&markers()), &identity));
    }

    #[test]
    fn discovers_a_real_tagged_process_through_the_real_proc() {
        use std::os::unix::process::CommandExt;
        let mut child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .env(ENV_AGENT_ID, "real-agent")
            .env(ENV_EXEC_ID, "real-exec")
            .env(ENV_INSTANCE, "real-instance")
            .spawn()
            .unwrap();

        let cmdline = cmdline(&["sleep", "30"]);
        let identity = Identity {
            agent_id: "real-agent",
            exec_id: "real-exec",
            instance: "real-instance",
            cmdline: &cmdline,
        };
        let found = discover_in(Path::new(PROC_ROOT), &[child.id()], own_uid(), &identity);

        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.root.map(|c| c.pid), Some(child.id()));
    }
}
