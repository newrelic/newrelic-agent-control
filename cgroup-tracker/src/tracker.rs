use crate::hierarchy::detect_base;
use crate::{CgroupName, CgroupOperation, CgroupTrackerError, Hierarchy};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::sys::stat::Mode;
use nix::unistd::{Pid, write};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PROCS_FILE: &str = "cgroup.procs";
const KILL_FILE: &str = "cgroup.kill";
const KILL_ATTEMPTS: u32 = 50;
const KILL_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// Creates, attaches to, kills and removes the cgroups of sub-agent executables.
#[derive(Debug, Clone)]
pub struct CgroupTracker {
    base: PathBuf,
    hierarchy: Hierarchy,
}

impl CgroupTracker {
    /// Detects the hierarchy and base cgroup from `/proc/self`.
    pub fn detect() -> Result<Self, CgroupTrackerError> {
        let open_file = |path: &str| {
            File::open(path)
                .map(BufReader::new)
                .map_err(CgroupTrackerError::io(
                    CgroupOperation::Open,
                    Path::new(path),
                ))
        };
        let (base, hierarchy) = detect_base(
            open_file("/proc/self/mountinfo")?,
            open_file("/proc/self/cgroup")?,
        )
        .map_err(CgroupTrackerError::Detect)?;
        Ok(Self::with_base(base, hierarchy))
    }

    /// Uses an explicit base directory.
    pub fn with_base(base: impl Into<PathBuf>, hierarchy: Hierarchy) -> Self {
        Self {
            base: base.into(),
            hierarchy,
        }
    }

    /// Base directory under which the tracker creates its cgroups.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Hierarchy kind the tracker operates on.
    pub fn hierarchy(&self) -> Hierarchy {
        self.hierarchy
    }

    /// Absolute path of the cgroup for `name`.
    pub fn path_of(&self, name: &CgroupName) -> PathBuf {
        self.base.join(name.relative_path())
    }

    /// Pids of every process in the cgroup and its descendant cgroups.
    /// Empty if the cgroup does not exist.
    pub fn members(&self, name: &CgroupName) -> Result<Vec<i32>, CgroupTrackerError> {
        member_pids(&self.path_of(name))
    }

    /// Creates the cgroup. Idempotent.
    pub fn create(&self, name: &CgroupName) -> Result<(), CgroupTrackerError> {
        let path = self.path_of(name);
        fs::create_dir_all(&path).map_err(CgroupTrackerError::io(CgroupOperation::Create, &path))
    }

    /// Creates the cgroup and configures `command` so the child joins it before `exec` and
    /// leads its own process group, keeping both invariants in a single place.
    pub fn prepare(
        &self,
        name: &CgroupName,
        command: &mut Command,
    ) -> Result<(), CgroupTrackerError> {
        self.create(name)?;
        let procs_path = self.path_of(name).join(PROCS_FILE);
        // Built before the fork so the child does not allocate.
        let procs = CString::new(procs_path.as_os_str().as_bytes()).map_err(|source| {
            CgroupTrackerError::InvalidPath {
                path: procs_path.clone(),
                source,
            }
        })?;
        command.process_group(0);
        // SAFETY: the closure only calls async-signal-safe syscalls (open, write, close) on a
        // `CString` built before the fork, so it neither allocates nor takes locks. Writing `0`
        // to `cgroup.procs` moves the calling process, so the child never execs outside the cgroup.
        unsafe {
            command.pre_exec(move || {
                let fd = nix::fcntl::open(
                    procs.as_c_str(),
                    nix::fcntl::OFlag::O_WRONLY | nix::fcntl::OFlag::O_CLOEXEC,
                    Mode::empty(),
                )?;
                write(&fd, b"0")?;
                Ok(())
            });
        }
        Ok(())
    }

    /// Kills every process in the cgroup (and its descendants), then removes it.
    ///
    /// Uses `cgroup.kill` when available and falls back to signalling the members.
    pub fn kill_and_remove(&self, name: &CgroupName) -> Result<(), CgroupTrackerError> {
        let path = self.path_of(name);
        for _ in 0..KILL_ATTEMPTS {
            self.kill_members(&path)?;
            match fs::remove_dir(&path) {
                Ok(()) => break,
                Err(err) if err.kind() == io::ErrorKind::NotFound => break,
                // Members are still around (e.g. dying or not yet reaped).
                Err(err) if err.kind() == io::ErrorKind::ResourceBusy => {
                    std::thread::sleep(KILL_RETRY_INTERVAL);
                    continue;
                }
                Err(err) => {
                    return Err(CgroupTrackerError::io(CgroupOperation::Remove, &path)(err));
                }
            }
        }
        if path.exists() {
            return Err(CgroupTrackerError::KillTimeout {
                path,
                attempts: KILL_ATTEMPTS,
            });
        }
        // Best effort: the parent (agent) cgroup may hold other executables.
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
        Ok(())
    }

    fn kill_members(&self, path: &Path) -> Result<(), CgroupTrackerError> {
        if self.hierarchy.supports_cgroup_kill() {
            match OpenOptions::new()
                .write(true)
                .open(path.join(KILL_FILE))
                .and_then(|mut f| f.write_all(b"1"))
            {
                Ok(()) => return Ok(()),
                // Missing cgroup, or a kernel older than 5.14: signal the members instead.
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(CgroupTrackerError::io(CgroupOperation::WriteKillFile, path)(err));
                }
            }
        }
        signal_members(path, &SystemSignaller)
    }
}

/// Sends SIGKILL to a process.
trait Signaller {
    fn sigkill(&self, pid: i32) -> Result<(), Errno>;
}

struct SystemSignaller;

impl Signaller for SystemSignaller {
    fn sigkill(&self, pid: i32) -> Result<(), Errno> {
        kill(Pid::from_raw(pid), Signal::SIGKILL)
    }
}

/// Signals every member of the cgroup. All pids are attempted, then the first failure
/// (other than an already exited process) is reported.
fn signal_members(path: &Path, signaller: &impl Signaller) -> Result<(), CgroupTrackerError> {
    let mut first_error = None;
    for pid in member_pids(path)? {
        match signaller.sigkill(pid) {
            Ok(()) => {}
            // The process exited in the meantime.
            Err(Errno::ESRCH) => {}
            Err(errno) => {
                tracing::debug!(pid, %errno, "Failed to kill cgroup member");
                first_error.get_or_insert((pid, errno));
            }
        }
    }
    first_error.map_or(Ok(()), |(pid, source)| {
        Err(CgroupTrackerError::Signal {
            pid,
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Pids of the cgroup and of its descendant cgroups.
fn member_pids(path: &Path) -> Result<Vec<i32>, CgroupTrackerError> {
    let procs_path = path.join(PROCS_FILE);
    let mut pids = match fs::read_to_string(&procs_path) {
        Ok(content) => content
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            return Err(CgroupTrackerError::io(CgroupOperation::Read, &procs_path)(
                err,
            ));
        }
    };
    for child in read_subdirs(path)? {
        pids.extend(member_pids(&child)?);
    }
    Ok(pids)
}

fn read_subdirs(path: &Path) -> Result<Vec<PathBuf>, CgroupTrackerError> {
    match fs::read_dir(path) {
        Ok(entries) => Ok(entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(CgroupTrackerError::io(CgroupOperation::Read, path)(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn tracker() -> (TempDir, CgroupTracker) {
        let dir = TempDir::new().unwrap();
        // V1 so killing goes through signals, a tempdir has no `cgroup.kill`.
        let tracker = CgroupTracker::with_base(dir.path(), Hierarchy::V1);
        (dir, tracker)
    }

    #[test]
    fn path_of_joins_base_and_name() {
        let tracker = CgroupTracker::with_base("/sys/fs/cgroup/test", Hierarchy::V2);
        assert_eq!(
            tracker.path_of(&CgroupName::new("agent", "exec")),
            PathBuf::from("/sys/fs/cgroup/test/_nr-ac-agent/exec")
        );
    }

    #[test]
    fn create_is_idempotent() {
        let (_dir, tracker) = tracker();
        let name = CgroupName::new("agent", "exec");
        tracker.create(&name).unwrap();
        tracker.create(&name).unwrap();
        assert!(tracker.path_of(&name).is_dir());
    }

    #[test]
    fn kill_and_remove_deletes_cgroup_and_empty_agent_dir() {
        let (_dir, tracker) = tracker();
        let name = CgroupName::new("agent", "exec");
        tracker.create(&name).unwrap();

        tracker.kill_and_remove(&name).unwrap();

        assert!(!tracker.path_of(&name).exists());
        assert!(!tracker.path_of(&name).parent().unwrap().exists());
    }

    #[test]
    fn kill_and_remove_keeps_agent_dir_with_other_executables() {
        let (_dir, tracker) = tracker();
        let (a, b) = (CgroupName::new("agent", "a"), CgroupName::new("agent", "b"));
        tracker.create(&a).unwrap();
        tracker.create(&b).unwrap();

        tracker.kill_and_remove(&a).unwrap();

        assert!(tracker.path_of(&b).is_dir());
    }

    #[test]
    fn kill_and_remove_missing_cgroup_is_ok() {
        let (_dir, tracker) = tracker();
        tracker
            .kill_and_remove(&CgroupName::new("agent", "exec"))
            .unwrap();
    }

    /// Records the signalled pids and fails the configured ones.
    struct FakeSignaller {
        failures: HashMap<i32, Errno>,
        calls: RefCell<Vec<i32>>,
    }

    impl FakeSignaller {
        fn failing_with(failures: impl IntoIterator<Item = (i32, Errno)>) -> Self {
            Self {
                failures: failures.into_iter().collect(),
                calls: RefCell::default(),
            }
        }
    }

    impl Signaller for FakeSignaller {
        fn sigkill(&self, pid: i32) -> Result<(), Errno> {
            self.calls.borrow_mut().push(pid);
            self.failures.get(&pid).map_or(Ok(()), |errno| Err(*errno))
        }
    }

    fn cgroup_with_members(pids: &[i32]) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let content: String = pids.iter().map(|p| format!("{p}\n")).collect();
        fs::write(dir.path().join(PROCS_FILE), content).unwrap();
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    #[test]
    fn signal_members_ignores_exited_processes() {
        let (_dir, path) = cgroup_with_members(&[10, 11]);
        let signaller = FakeSignaller::failing_with([(10, Errno::ESRCH)]);

        signal_members(&path, &signaller).unwrap();

        assert_eq!(signaller.calls.take(), vec![10, 11]);
    }

    #[test]
    fn signal_members_reports_first_failure_after_signalling_all() {
        let (_dir, path) = cgroup_with_members(&[10, 11, 12]);
        let signaller = FakeSignaller::failing_with([(10, Errno::EPERM), (11, Errno::EINVAL)]);

        let err = signal_members(&path, &signaller).unwrap_err();

        // Every pid is attempted even though the first one fails.
        assert_eq!(signaller.calls.take(), vec![10, 11, 12]);
        assert_matches!(err, CgroupTrackerError::Signal { pid: 10, source, .. } => {
            assert_eq!(source, Errno::EPERM);
        });
    }

    #[test]
    fn members_includes_descendant_cgroups() {
        let (_dir, tracker) = tracker();
        let name = CgroupName::new("agent", "exec");
        tracker.create(&name).unwrap();
        let path = tracker.path_of(&name);
        let child = path.join("child");
        fs::create_dir(&child).unwrap();
        fs::write(path.join(PROCS_FILE), "10\n11\n").unwrap();
        fs::write(child.join(PROCS_FILE), "12\n").unwrap();

        let mut pids = tracker.members(&name).unwrap();
        pids.sort_unstable();

        assert_eq!(pids, vec![10, 11, 12]);
    }

    #[test]
    fn members_is_empty_when_cgroup_does_not_exist() {
        let (_dir, tracker) = tracker();

        let pids = tracker
            .members(&CgroupName::new("agent", "missing"))
            .unwrap();

        assert!(pids.is_empty());
    }

    #[test]
    fn prepare_attaches_child_before_exec() {
        let (_dir, tracker) = tracker();
        let name = CgroupName::new("agent", "exec");
        tracker.create(&name).unwrap();
        // A regular file stands in for the kernel interface file.
        let procs = tracker.path_of(&name).join(PROCS_FILE);
        fs::write(&procs, "").unwrap();

        let mut command = Command::new("true");
        tracker.prepare(&name, &mut command).unwrap();
        assert!(command.status().unwrap().success());

        assert_eq!(fs::read_to_string(procs).unwrap(), "0");
    }

    #[test]
    fn prepare_fails_to_spawn_when_attach_fails() {
        let (_dir, tracker) = tracker();
        let mut command = Command::new("true");
        // No `cgroup.procs` file, so attaching fails and the command must not run.
        tracker
            .prepare(&CgroupName::new("agent", "exec"), &mut command)
            .unwrap();
        assert!(command.spawn().is_err());
    }
}
