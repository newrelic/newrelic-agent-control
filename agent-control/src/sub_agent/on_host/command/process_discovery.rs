//! PoC, Linux only: finds a still-running process a previous Agent Control instance spawned for
//! an `(agent_id, exec_id)` pair, without any persisted bookkeeping.
//!
//! Every spawned executable is tagged with [`ENV_AGENT_ID`] and [`ENV_EXEC_ID`]. After a crash
//! (`KillMode=process` leaves the sub-agents running in the unit's cgroup), the restarted
//! instance scans its own cgroup for processes carrying both markers and the expected command
//! line, and adopts the top-level one.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Env var carrying the owning agent id; set on every executable Agent Control spawns.
pub const ENV_AGENT_ID: &str = "NR_AC_AGENT_ID";
/// Env var carrying the owning executable id; set on every executable Agent Control spawns.
pub const ENV_EXEC_ID: &str = "NR_AC_EXEC_ID";

const PROC_ROOT: &str = "/proc";
const CGROUP_MOUNT: &str = "/sys/fs/cgroup";

/// A live process that looks like the executable Agent Control spawned for an `(agent, exec)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub pid: u32,
    pub ppid: u32,
    /// `starttime` from `/proc/<pid>/stat`, used to detect pid reuse after discovery.
    pub start_time_marker: u64,
}

/// Looks for a process in Agent Control's own cgroup that was spawned for `(agent_id, exec_id)`
/// with exactly the `expected_cmdline` (program followed by args) and is owned by the same user.
///
/// Returns `None` when nothing matches, or when the cgroup can't be determined (cgroup v1
/// hosts, no `/sys/fs/cgroup`), in which case Agent Control just spawns as usual.
pub fn discover(
    agent_id: &str,
    exec_id: &str,
    expected_cmdline: &[OsString],
) -> Option<Candidate> {
    let pids = own_cgroup_pids()?;
    discover_in(
        Path::new(PROC_ROOT),
        &pids,
        nix::unistd::geteuid().as_raw(),
        agent_id,
        exec_id,
        expected_cmdline,
    )
}

/// Pids of every process currently in Agent Control's own cgroup (cgroup v2 only).
pub fn own_cgroup_pids() -> Option<Vec<u32>> {
    let self_cgroup = std::fs::read_to_string(format!("{PROC_ROOT}/self/cgroup")).ok()?;
    let procs_path = cgroup_procs_path(&self_cgroup, Path::new(CGROUP_MOUNT))?;
    let procs = std::fs::read_to_string(procs_path).ok()?;
    Some(procs.lines().filter_map(|l| l.trim().parse().ok()).collect())
}

/// Resolves the `cgroup.procs` file from the contents of `/proc/self/cgroup`. Only the
/// unified-hierarchy entry (`0::<path>`) is considered.
fn cgroup_procs_path(self_cgroup: &str, mount: &Path) -> Option<PathBuf> {
    let relative = self_cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))?;
    Some(
        mount
            .join(relative.trim().trim_start_matches('/'))
            .join("cgroup.procs"),
    )
}

/// [`discover`] against an arbitrary `/proc` root and candidate pid list, so it can be tested
/// against a fake `/proc` tree.
fn discover_in(
    proc_root: &Path,
    pids: &[u32],
    uid: u32,
    agent_id: &str,
    exec_id: &str,
    expected_cmdline: &[OsString],
) -> Option<Candidate> {
    let candidates: Vec<Candidate> = pids
        .iter()
        .filter_map(|&pid| {
            read_candidate(proc_root, pid, uid, agent_id, exec_id, expected_cmdline)
        })
        .collect();
    select_root(&candidates).cloned()
}

/// Picks the top-level process among `candidates`: the one whose parent is not itself a
/// candidate, so a forked worker carrying the same markers and command line isn't adopted in
/// place of its parent. If several remain, the oldest wins.
fn select_root(candidates: &[Candidate]) -> Option<&Candidate> {
    let pids: HashSet<u32> = candidates.iter().map(|c| c.pid).collect();
    candidates
        .iter()
        .filter(|c| !pids.contains(&c.ppid))
        .min_by_key(|c| c.start_time_marker)
}

fn read_candidate(
    proc_root: &Path,
    pid: u32,
    uid: u32,
    agent_id: &str,
    exec_id: &str,
    expected_cmdline: &[OsString],
) -> Option<Candidate> {
    let dir = proc_root.join(pid.to_string());

    if std::fs::metadata(&dir).ok()?.uid() != uid {
        return None;
    }
    if !has_markers(&std::fs::read(dir.join("environ")).ok()?, agent_id, exec_id) {
        return None;
    }
    if !cmdline_matches(&std::fs::read(dir.join("cmdline")).ok()?, expected_cmdline) {
        return None;
    }
    let (ppid, start_time_marker) = parse_stat(&std::fs::read_to_string(dir.join("stat")).ok()?)?;
    Some(Candidate {
        pid,
        ppid,
        start_time_marker,
    })
}

/// Whether the NUL-separated `environ` carries both markers with exactly these values.
fn has_markers(environ: &[u8], agent_id: &str, exec_id: &str) -> bool {
    let agent = format!("{ENV_AGENT_ID}={agent_id}");
    let exec = format!("{ENV_EXEC_ID}={exec_id}");
    let mut found = (false, false);
    for entry in environ.split(|b| *b == 0) {
        found.0 |= entry == agent.as_bytes();
        found.1 |= entry == exec.as_bytes();
    }
    found.0 && found.1
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

/// Extracts `(ppid, starttime)` from the contents of `/proc/<pid>/stat`.
fn parse_stat(stat: &str) -> Option<(u32, u64)> {
    // `comm` (field 2) is parenthesized and may itself contain spaces or parens, so split on
    // the *last* ')' rather than whitespace to find where the numeric fields resume.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // `state` (field 3) is index 0, so `ppid` (field 4) is index 1 and `starttime` (field 22)
    // is index 19.
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const AGENT: &str = "agent-a";
    const EXEC: &str = "exec-a";

    fn cmdline(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn stat_line(comm: &str, ppid: u32, starttime: u64) -> String {
        // pid (comm) state ppid + 16 fields up to starttime (field 22).
        let mid = vec!["0"; 16].join(" ");
        format!("1 ({comm}) S {ppid} {mid} {starttime} 0 0")
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

        fn add(&self, pid: u32, env: &[&str], argv: &[&str], ppid: u32, starttime: u64) {
            let dir = self.dir.path().join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("environ"), nul_joined(env)).unwrap();
            std::fs::write(dir.join("cmdline"), nul_joined(argv)).unwrap();
            std::fs::write(dir.join("stat"), stat_line("sleep", ppid, starttime)).unwrap();
        }

        fn discover(&self, pids: &[u32], uid: u32, expected: &[&str]) -> Option<Candidate> {
            discover_in(self.dir.path(), pids, uid, AGENT, EXEC, &cmdline(expected))
        }
    }

    fn nul_joined(parts: &[&str]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
    }

    fn own_uid() -> u32 {
        nix::unistd::geteuid().as_raw()
    }

    fn markers() -> [&'static str; 2] {
        ["NR_AC_AGENT_ID=agent-a", "NR_AC_EXEC_ID=exec-a"]
    }

    #[test]
    fn parse_stat_handles_parens_and_spaces_in_comm() {
        let stat = stat_line("we) (ird name", 42, 987);
        assert_eq!(parse_stat(&stat), Some((42, 987)));
    }

    #[test]
    fn cgroup_procs_path_uses_the_unified_hierarchy_entry() {
        let self_cgroup = "12:cpu:/legacy\n0::/system.slice/newrelic-agent-control.service\n";
        assert_eq!(
            cgroup_procs_path(self_cgroup, Path::new("/sys/fs/cgroup")),
            Some(PathBuf::from(
                "/sys/fs/cgroup/system.slice/newrelic-agent-control.service/cgroup.procs"
            ))
        );
        assert_eq!(
            cgroup_procs_path("0::/\n", Path::new("/sys/fs/cgroup")),
            Some(PathBuf::from("/sys/fs/cgroup/cgroup.procs"))
        );
    }

    #[test]
    fn cgroup_procs_path_is_none_without_a_unified_hierarchy() {
        assert_eq!(
            cgroup_procs_path("1:name=systemd:/x\n", Path::new("/sys/fs/cgroup")),
            None
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
                start_time_marker: 500
            })
        );
    }

    #[test]
    fn ignores_processes_for_another_agent_or_exec() {
        let proc = FakeProc::new();
        proc.add(
            10,
            &["NR_AC_AGENT_ID=other", "NR_AC_EXEC_ID=exec-a"],
            &["/bin/sleep", "30"],
            1,
            500,
        );
        proc.add(
            11,
            &["NR_AC_AGENT_ID=agent-a", "NR_AC_EXEC_ID=other"],
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

        assert_eq!(
            proc.discover(&[10], own_uid(), &["/bin/sleep", "30"]),
            None
        );
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
    fn prefers_the_parent_over_a_forked_worker_with_the_same_markers() {
        let proc = FakeProc::new();
        // The worker is older by start time, to prove the parent/child relation decides, not age.
        proc.add(20, &markers(), &["/bin/sleep", "30"], 10, 100);
        proc.add(10, &markers(), &["/bin/sleep", "30"], 1, 500);

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
        assert!(!has_markers(
            &nul_joined(&["NR_AC_AGENT_ID=agent-ab", "NR_AC_EXEC_ID=exec-a"]),
            AGENT,
            EXEC
        ));
        assert!(has_markers(&nul_joined(&markers()), AGENT, EXEC));
    }

    #[test]
    fn discovers_a_real_tagged_process_through_the_real_proc() {
        let mut child = Command::new("sleep")
            .arg("30")
            .env(ENV_AGENT_ID, "real-agent")
            .env(ENV_EXEC_ID, "real-exec")
            .spawn()
            .unwrap();

        let found = discover_in(
            Path::new(PROC_ROOT),
            &[child.id()],
            own_uid(),
            "real-agent",
            "real-exec",
            &cmdline(&["sleep", "30"]),
        );

        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.map(|c| c.pid), Some(child.id()));
    }
}
