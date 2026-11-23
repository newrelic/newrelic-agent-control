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
use std::process::{Child, Command};
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

    /// Uses an explicit base directory and hierarchy, skipping detection.
    ///
    /// Useful for non-standard cgroup mounts, resuming from a previously detected base, or
    /// tests using a temporary directory. The base is not validated.
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

    /// Handle to the cgroup for `name`. The cgroup is not created until needed.
    pub fn cgroup<'a>(&'a self, name: &'a CgroupName) -> Cgroup<'a> {
        Cgroup {
            tracker: self,
            name,
        }
    }
}

/// A cgroup of a [CgroupTracker], identified by its [CgroupName]. It may not exist on disk yet.
#[derive(Debug, Clone, Copy)]
pub struct Cgroup<'a> {
    tracker: &'a CgroupTracker,
    name: &'a CgroupName,
}

impl Cgroup<'_> {
    /// Absolute path of the cgroup. It may not exist yet, see [Self::prepare].
    pub fn path(&self) -> PathBuf {
        self.tracker.base.join(self.name.relative_path())
    }

    /// Pids of every process in the cgroup and its descendant cgroups.
    /// Empty if the cgroup does not exist.
    pub fn members(&self) -> Result<Vec<i32>, CgroupTrackerError> {
        member_pids(&self.path())
    }

    /// Creates the cgroup. Idempotent.
    fn create(&self) -> Result<(), CgroupTrackerError> {
        let path = self.path();
        fs::create_dir_all(&path).map_err(CgroupTrackerError::io(CgroupOperation::Create, &path))
    }

    /// Creates the cgroup and configures `command` so the child joins it before `exec` and
    /// leads its own process group, keeping both invariants in a single place.
    pub fn prepare(&self, command: &mut Command) -> Result<(), CgroupTrackerError> {
        self.create()?;
        let procs_path = self.path().join(PROCS_FILE);
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
        // NOTE: any change on the closure passed to `pre_exec` needs to keep it free of
        // locks and/or allocations, otherwise it could cause deadlocks on the child.
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

    /// Creates the cgroup and spawns `command` inside it, see [Self::prepare].
    ///
    /// If the spawn fails, the cgroup is left in place (empty if it was created by this call).
    /// Callers should clean it up with [Self::kill_and_remove] if they do not retry.
    pub fn spawn(&self, mut command: Command) -> Result<Child, CgroupTrackerError> {
        self.prepare(&mut command)?;
        command.spawn().map_err(|source| CgroupTrackerError::Spawn {
            program: command.get_program().to_string_lossy().into_owned(),
            path: self.path(),
            source,
        })
    }

    /// Kills every process in the cgroup (and its descendants), then removes it along with
    /// its descendant cgroups.
    ///
    /// Uses `cgroup.kill` when available and falls back to signalling the members.
    pub fn kill_and_remove(&self) -> Result<(), CgroupTrackerError> {
        let path = self.path();
        for _ in 0..KILL_ATTEMPTS {
            self.kill_members(&path)?;
            match remove_cgroup_tree(&path) {
                Ok(()) => break,
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
        // Best effort: the shared root cgroup is only removed once no other cgroup is left in it.
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
        Ok(())
    }

    fn kill_members(&self, path: &Path) -> Result<(), CgroupTrackerError> {
        if self.tracker.hierarchy.supports_cgroup_kill() {
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

/// Removes the cgroup and its descendant cgroups, deepest first, since the kernel refuses to
/// remove a cgroup that still has children. A missing cgroup is not an error.
fn remove_cgroup_tree(path: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_cgroup_tree(&entry.path())?;
        }
    }
    match fs::remove_dir(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
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

    fn name(value: &str) -> CgroupName {
        CgroupName::try_from(value).unwrap()
    }

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
            tracker.cgroup(&name("agent-exec")).path(),
            PathBuf::from("/sys/fs/cgroup/test/_nr-ac/agent-exec")
        );
    }

    #[test]
    fn create_is_idempotent() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        tracker.cgroup(&name).create().unwrap();
        assert!(tracker.cgroup(&name).path().is_dir());
    }

    #[test]
    fn spawn_failure_returns_spawn_error_and_keeps_cgroup() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");

        let err = tracker
            .cgroup(&name)
            .spawn(Command::new("/nonexistent/binary"))
            .unwrap_err();

        assert_matches!(err, CgroupTrackerError::Spawn { program, path, .. } => {
            assert_eq!(program, "/nonexistent/binary");
            assert_eq!(path, tracker.cgroup(&name).path());
        });
        assert!(tracker.cgroup(&name).path().is_dir());
    }

    #[test]
    fn kill_and_remove_deletes_cgroup_and_empty_root_dir() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();

        tracker.cgroup(&name).kill_and_remove().unwrap();

        assert!(!tracker.cgroup(&name).path().exists());
        assert!(!tracker.cgroup(&name).path().parent().unwrap().exists());
    }

    #[test]
    fn kill_and_remove_keeps_root_dir_with_other_cgroups() {
        let (_dir, tracker) = tracker();
        let (a, b) = (name("agent-a"), name("agent-b"));
        tracker.cgroup(&a).create().unwrap();
        tracker.cgroup(&b).create().unwrap();

        tracker.cgroup(&a).kill_and_remove().unwrap();

        assert!(tracker.cgroup(&b).path().is_dir());
    }

    #[test]
    fn kill_and_remove_deletes_descendant_cgroups() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        let nested = tracker
            .cgroup(&name)
            .path()
            .join("child")
            .join("grandchild");
        fs::create_dir_all(&nested).unwrap();

        tracker.cgroup(&name).kill_and_remove().unwrap();

        assert!(!tracker.cgroup(&name).path().exists());
    }

    #[test]
    fn kill_and_remove_missing_cgroup_is_ok() {
        let (_dir, tracker) = tracker();
        tracker
            .cgroup(&name("agent-exec"))
            .kill_and_remove()
            .unwrap();
    }

    #[test]
    fn kill_and_remove_on_v2_without_kill_file_falls_back_to_signals() {
        // A tempdir has no `cgroup.kill`, like a kernel older than 5.14.
        let dir = TempDir::new().unwrap();
        let tracker = CgroupTracker::with_base(dir.path(), Hierarchy::V2);
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        assert!(!tracker.cgroup(&name).path().join(KILL_FILE).exists());

        tracker.cgroup(&name).kill_and_remove().unwrap();

        assert!(!tracker.cgroup(&name).path().exists());
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
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        let path = tracker.cgroup(&name).path();
        let child = path.join("child");
        fs::create_dir(&child).unwrap();
        fs::write(path.join(PROCS_FILE), "10\n11\n").unwrap();
        fs::write(child.join(PROCS_FILE), "12\n").unwrap();

        let mut pids = tracker.cgroup(&name).members().unwrap();
        pids.sort_unstable();

        assert_eq!(pids, vec![10, 11, 12]);
    }

    #[test]
    fn members_is_empty_when_cgroup_does_not_exist() {
        let (_dir, tracker) = tracker();

        let pids = tracker.cgroup(&name("missing")).members().unwrap();

        assert!(pids.is_empty());
    }

    #[test]
    fn prepare_attaches_child_before_exec() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        // A regular file stands in for the kernel interface file.
        let procs = tracker.cgroup(&name).path().join(PROCS_FILE);
        fs::write(&procs, "").unwrap();

        let mut command = Command::new("true");
        tracker.cgroup(&name).prepare(&mut command).unwrap();
        assert!(command.status().unwrap().success());

        assert_eq!(fs::read_to_string(procs).unwrap(), "0");
    }

    #[test]
    fn spawn_runs_command_attached_to_the_cgroup() {
        let (_dir, tracker) = tracker();
        let name = name("agent-exec");
        tracker.cgroup(&name).create().unwrap();
        // A regular file stands in for the kernel interface file.
        let procs = tracker.cgroup(&name).path().join(PROCS_FILE);
        fs::write(&procs, "").unwrap();

        let mut child = tracker.cgroup(&name).spawn(Command::new("true")).unwrap();

        assert!(child.wait().unwrap().success());
        assert_eq!(fs::read_to_string(procs).unwrap(), "0");
    }

    #[test]
    fn prepare_fails_to_spawn_when_attach_fails() {
        let (_dir, tracker) = tracker();
        let mut command = Command::new("true");
        // No `cgroup.procs` file, so attaching fails and the command must not run.
        tracker
            .cgroup(&name("agent-exec"))
            .prepare(&mut command)
            .unwrap();
        assert!(command.spawn().is_err());
    }
}
