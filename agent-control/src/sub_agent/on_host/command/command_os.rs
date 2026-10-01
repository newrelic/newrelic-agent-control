//! OS-level command wrapper with not-started/started states, output streaming, and shutdown handling.

#[cfg(target_os = "linux")]
use tracing::info;
use tracing::warn;

#[cfg(target_os = "linux")]
use super::process_record;
use super::{
    error::CommandError,
    logging::{
        self,
        file_logger::{FileSystemLoggers, file_logger},
        logger::Logger,
    },
};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_FILE_NAME_SUFFIX, STDOUT_LOG_FILE_NAME_SUFFIX};
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::logging::file_logger::SubAgentFileLoggingConfig;
#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
/// A configured but not-yet-spawned OS command.
pub struct CommandOSNotStarted {
    cmd: Command,
    agent_id: AgentID,
    file_logging_config: SubAgentFileLoggingConfig,
    shutdown_timeout: Duration,
    /// PoC: defaults to the real `runtime-state`-rooted storer; overridable in tests via
    /// [`CommandOSNotStarted::with_process_record_storer`] so adoption can be driven against
    /// a temp directory instead of Agent Control's real dynamic data dir.
    #[cfg(target_os = "linux")]
    process_record_storer: Arc<dyn process_record::ProcessRecordStorer + Send + Sync>,
}
/// A spawned OS command process with its loggers and (on Windows) job object.
pub struct CommandOSStarted {
    agent_id: AgentID,
    process: ManagedProcess,
    loggers: Option<FileSystemLoggers>,
    shutdown_timeout: Duration,

    #[cfg(target_family = "windows")]
    job_object: Option<JobObject>,
}

/// PoC: a process this `CommandOSStarted` supervises, either one Agent Control spawned this
/// run (`Spawned`, a real `Child`) or one it adopted from a previous instance's bookkeeping
/// (`Adopted`; see `process_record`). Kept as a thin wrapper mirroring `Child`'s API so
/// `CommandOSStarted`'s own methods barely change either way.
enum ManagedProcess {
    Spawned(Child),
    // Adoption is wired in Linux-only (see `start()`); never constructed elsewhere.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Adopted(AdoptedProcess),
}

impl ManagedProcess {
    fn id(&self) -> u32 {
        match self {
            Self::Spawned(child) => child.id(),
            Self::Adopted(adopted) => adopted.pid,
        }
    }

    /// Mirrors `Child::try_wait`. For an adopted process this can never be a real
    /// `waitpid()`-backed result (we're not its parent); it's a liveness poll that
    /// fabricates an [`ExitStatus`] once the process disappears or its start-time marker no
    /// longer matches, since there's no way to retrieve its real exit code either way.
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self {
            Self::Spawned(child) => child.try_wait(),
            Self::Adopted(adopted) => Ok(if adopted.is_alive() {
                None
            } else {
                Some(unknown_exit_status())
            }),
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        match self {
            Self::Spawned(child) => child.wait(),
            Self::Adopted(_) => {
                while self.try_wait()?.is_none() {
                    std::thread::sleep(POLL_INTERVAL);
                }
                // Safe: the loop above only exits once try_wait returned Some(_).
                Ok(self.try_wait()?.expect("process no longer alive"))
            }
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        match self {
            Self::Spawned(child) => child.kill(),
            Self::Adopted(adopted) => adopted.kill(),
        }
    }

    /// Only ever `Some` for a process this run actually spawned; an adopted process never
    /// had its stdout piped to us in the first place.
    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        match self {
            Self::Spawned(child) => child.stdout.take(),
            Self::Adopted(_) => None,
        }
    }

    fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        match self {
            Self::Spawned(child) => child.stderr.take(),
            Self::Adopted(_) => None,
        }
    }
}

/// PoC: an already-running process Agent Control didn't spawn this run, recognized from a
/// previous instance's bookkeeping. Linux only for now; liveness is a poll against
/// `/proc/<pid>/stat`, not an event, since we're not this process's real parent and can't
/// `waitpid()` on it.
struct AdoptedProcess {
    pid: u32,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    start_time_marker: u64,
}

impl AdoptedProcess {
    fn is_alive(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            process_record::read_proc_start_time_marker(self.pid) == Some(self.start_time_marker)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[cfg(target_family = "unix")]
    fn kill(&self) -> io::Result<()> {
        use nix::sys::signal;
        use nix::unistd::Pid;
        signal::kill(Pid::from_raw(self.pid as i32), signal::SIGKILL).map_err(std::io::Error::from)
    }

    #[cfg(target_family = "windows")]
    fn kill(&self) -> io::Result<()> {
        // PoC: Windows adoption isn't wired in yet; nothing constructs `Adopted` there.
        Err(io::Error::other(
            "killing an adopted process is not implemented on Windows",
        ))
    }
}

/// Fabricated [`ExitStatus`] for an adopted process whose real exit code we have no way to
/// retrieve (we were never its parent, so `waitpid()` was never ours to call). Callers must
/// not treat this as the process's actual exit code, only as "it's gone".
fn unknown_exit_status() -> ExitStatus {
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
    #[cfg(target_family = "windows")]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
}

/// PoC, Linux only: checks the bookkeeping for `agent_id` and, if it points at a pid that's
/// still alive with a matching start-time marker, returns it as adoptable. Takes the storer
/// as a parameter (rather than constructing one internally) specifically so this is
/// testable against a temp-dir storer instead of Agent Control's real dynamic data dir.
#[cfg(target_os = "linux")]
fn adoptable_process(
    agent_id: &AgentID,
    storer: &dyn process_record::ProcessRecordStorer,
) -> Option<AdoptedProcess> {
    let record = storer.get(agent_id).ok().flatten()?;
    let current_marker = process_record::read_proc_start_time_marker(record.pid)?;
    if current_marker != record.start_time_marker {
        return None;
    }
    Some(AdoptedProcess {
        pid: record.pid,
        start_time_marker: record.start_time_marker,
    })
}

/// PoC, Linux only: persists the bookkeeping for a process this run just spawned, so a
/// future Agent Control restart can recognize it. Best-effort: a failure to persist only
/// means a future restart won't be able to adopt this process, not that this run's spawn
/// itself failed.
#[cfg(target_os = "linux")]
fn record_spawned_process(
    agent_id: &AgentID,
    pid: u32,
    storer: &dyn process_record::ProcessRecordStorer,
) {
    let Some(start_time_marker) = process_record::read_proc_start_time_marker(pid) else {
        warn!(%agent_id, pid, "PoC: could not read start-time marker for freshly spawned process, adoption bookkeeping skipped");
        return;
    };
    let record = process_record::ProcessRecord {
        pid,
        start_time_marker,
    };
    if let Err(err) = storer.set(agent_id, &record) {
        warn!(%agent_id, pid, "PoC: failed to persist process record: {err}");
    }
}

/// PoC: the real `runtime-state`-rooted storer, used as [`CommandOSNotStarted::new`]'s
/// default. A real implementation would inject this the same way `DataStore`/
/// `InstanceIDStorer` are injected elsewhere, instead of constructing it here.
#[cfg(target_os = "linux")]
fn default_process_record_storer() -> Arc<dyn process_record::ProcessRecordStorer + Send + Sync> {
    use crate::agent_control::defaults::AGENT_CONTROL_DATA_DIR;

    Arc::new(process_record::FileProcessRecordStorer::new(
        fs::file::LocalFile,
        fs::directory_manager::DirectoryManagerFs,
        std::path::PathBuf::from(AGENT_CONTROL_DATA_DIR),
    ))
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
impl CommandOSNotStarted {
    /// Creates a not-started command for the given executable, configuring stdio pipes and logging.
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        file_logging_config: SubAgentFileLoggingConfig,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args)
            .envs(&executable_data.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            agent_id,
            cmd,
            file_logging_config,
            shutdown_timeout: executable_data.shutdown_timeout,
            #[cfg(target_os = "linux")]
            process_record_storer: default_process_record_storer(),
        }
    }

    /// PoC, Linux only, test-only: overrides the process-record storer used for adoption,
    /// so tests can drive it against a temp directory instead of Agent Control's real
    /// dynamic data dir.
    #[cfg(all(test, target_os = "linux"))]
    fn with_process_record_storer(
        mut self,
        storer: Arc<dyn process_record::ProcessRecordStorer + Send + Sync>,
    ) -> Self {
        self.process_record_storer = storer;
        self
    }

    /// Spawns the process, setting up file loggers and (on Windows) a job object.
    ///
    /// PoC, Linux only: before spawning, checks whether a previous Agent Control instance's
    /// bookkeeping points at a still-running process for this `agent_id`; if so, adopts it
    /// instead of spawning a duplicate. See the on-host crash-survival CDD.
    pub fn start(mut self) -> Result<CommandOSStarted, CommandError> {
        #[cfg(target_os = "linux")]
        if let Some(adopted) =
            adoptable_process(&self.agent_id, self.process_record_storer.as_ref())
        {
            info!(agent_id = %self.agent_id, pid = adopted.pid, "PoC: adopted a still-running process from a previous Agent Control instance instead of respawning it");
            return Ok(CommandOSStarted {
                agent_id: self.agent_id,
                process: ManagedProcess::Adopted(adopted),
                loggers: None,
                shutdown_timeout: self.shutdown_timeout,
            });
        }

        let loggers = if self.file_logging_config.enabled {
            Some(FileSystemLoggers::new(
                file_logger(
                    &self.agent_id,
                    self.file_logging_config.clone(),
                    STDOUT_LOG_FILE_NAME_SUFFIX,
                )?,
                file_logger(
                    &self.agent_id,
                    self.file_logging_config.clone(),
                    STDERR_LOG_FILE_NAME_SUFFIX,
                )?,
            ))
        } else {
            None
        };
        let child = self.cmd.spawn()?;

        #[cfg(target_os = "linux")]
        record_spawned_process(
            &self.agent_id,
            child.id(),
            self.process_record_storer.as_ref(),
        );

        #[cfg(target_family = "unix")]
        {
            Ok(CommandOSStarted {
                agent_id: self.agent_id,
                process: ManagedProcess::Spawned(child),
                loggers,
                shutdown_timeout: self.shutdown_timeout,
            })
        }
        #[cfg(target_family = "windows")]
        {
            // Each started process gets its own JobObject. All sub-processes that the process spawns
            // will be assigned to the same JobObject, allowing for a graceful shutdown of the entire process tree.
            let job_object = JobObject::new()?;
            job_object.assign_process(&child)?;
            Ok(CommandOSStarted {
                agent_id: self.agent_id,
                process: ManagedProcess::Spawned(child),
                job_object: Some(job_object),
                loggers,
                shutdown_timeout: self.shutdown_timeout,
            })
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started Command OS
////////////////////////////////////////////////////////////////////////////////////

impl CommandOSStarted {
    /// Returns the process id of the running command.
    pub fn get_pid(&self) -> u32 {
        self.process.id()
    }

    /// Returns whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        self.process.try_wait().is_ok_and(|v| v.is_none())
    }

    pub(crate) fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.process.wait().map_err(CommandError::from)
    }

    /// Drains piped stdout/stderr into AC's own log stream. A no-op for a process whose
    /// output isn't piped to us in the first place (an adopted process, or one spawned with
    /// file-redirected stdio): there's nothing to drain, and that's expected, not an error.
    pub(crate) fn stream(mut self) -> Result<Self, CommandError> {
        let (Some(stdout), Some(stderr)) = (self.process.take_stdout(), self.process.take_stderr())
        else {
            return Ok(self);
        };

        let mut stdout_loggers = vec![Logger::Stdout(self.agent_id.clone())];
        let mut stderr_loggers = vec![Logger::Stderr(self.agent_id.clone())];

        if let Some(l) = self.loggers.take() {
            let (out, err) = l.into_loggers();
            stdout_loggers.push(Logger::File(Box::new(out)));
            stderr_loggers.push(Logger::File(Box::new(err)));
        };

        // Read stdout and send to the channel
        logging::thread::spawn_logger(stdout, stdout_loggers);

        // Read stderr and send to the channel
        logging::thread::spawn_logger(stderr, stderr_loggers);

        Ok(self)
    }

    fn is_running_after_timeout(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if !self.is_running() {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        true
    }

    /// Stops the process, attempting a graceful shutdown and forcing a kill after the timeout.
    pub fn shutdown(&mut self) -> Result<(), CommandError> {
        // Attempt a graceful shutdown (platform-dependent).
        let graceful_shutdown_result = self.graceful_shutdown();

        if let Err(e) = &graceful_shutdown_result {
            warn!(agent_id = %self.agent_id, "Graceful shutdown failed for process {}: {e}",self.get_pid());
        }

        if graceful_shutdown_result.is_err() || self.is_running_after_timeout(self.shutdown_timeout)
        {
            self.process.kill().map_err(CommandError::from)?;
        }

        Ok(())
    }

    #[cfg(target_family = "unix")]
    fn graceful_shutdown(&self) -> Result<(), CommandError> {
        use nix::{sys::signal, unistd::Pid};
        let pid = self.get_pid();

        signal::kill(Pid::from_raw(pid as i32), signal::SIGTERM)
            .map_err(|e| CommandError::from(std::io::Error::from(e)))
    }

    #[cfg(target_family = "windows")]
    /// On Windows there is no direct equivalent to sending SIGTERM. Applications that runs as
    /// services handles stops signals via Service Control Manager (SCM), and console applications
    /// can handle Ctrl-C or Ctrl-Break events via attached consoles.
    /// The current implementation uses Job Objects to manage process groups, and there is no graceful
    /// shutdown signal sent to the processes. The Job Object will terminate all associated processes.
    fn graceful_shutdown(&mut self) -> Result<(), CommandError> {
        if let Some(job_object) = self.job_object.take() {
            job_object.kill()?;
        }
        Ok(())
    }
}

/// PoC: standalone repro for the on-host crash-survival CDD's logging hazard. Not an
/// AC-integration test, this is deliberately bare `std::process` to isolate the OS/std
/// behavior the CDD's design depends on, independent of anything AC-specific.
///
/// Rewriting AC's actual sub-agent logging pipeline (`FileLogger`, built on
/// `tracing_appender`'s rotating file writer, fed formatted lines from the pipe-reading
/// thread) to redirect a child's stdio to a plain file instead of a pipe is real, separate
/// work the CDD already scopes out, not something this PoC attempts. This only proves the
/// specific claim that motivates that work: closing a piped child's stdout read end without
/// the child's cooperation is not safe to do casually.
#[cfg(all(test, target_family = "unix"))]
mod sigpipe_poc {
    use std::process::{Command, Stdio};

    /// Confirms that closing a piped child's stdout read end (without the child's
    /// cooperation) delivers it a fatal `SIGPIPE` on its next write, the exact hazard the
    /// CDD's Logging section describes for "spawn then forget" without switching away from
    /// `Stdio::piped()` first.
    ///
    /// Windows has no `SIGPIPE` equivalent: a write to a broken pipe there fails with an
    /// `ERROR_BROKEN_PIPE`-flavored error return instead of being forcibly signaled, so
    /// whether the child dies depends on whether it handles that error, not on the OS. This
    /// test is Unix-only because the hazard it demonstrates is a Unix-specific one.
    #[test]
    fn closing_piped_stdout_sigpipes_the_writer() {
        use nix::sys::signal::Signal;
        use std::os::unix::process::ExitStatusExt;

        // `yes` writes to stdout in a tight loop and is a real executable, not a shell
        // builtin, so its signal handling isn't muddied by the shell's own builtin dispatch.
        let mut child = Command::new("yes")
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn `yes`");

        // Close the read end without ever draining it, the "spawn then forget by dropping
        // the pipe" case the CDD flags as unsafe, while still holding `child` itself so we
        // can observe how it died afterward.
        drop(child.stdout.take());

        let status = child.wait().expect("failed to wait on `yes`");

        assert_eq!(
            status.signal(),
            Some(Signal::SIGPIPE as i32),
            "expected `yes` to be killed by SIGPIPE once its stdout pipe closed, got: {status:?}"
        );
    }
}

/// PoC: drives the adoption feature itself, answering "is this testable at all" concretely
/// rather than leaving it as an unexercised code path. Two levels: the bookkeeping-matching
/// logic directly (`adoptable_process`/`record_spawned_process`), and the real call site
/// (`CommandOSNotStarted::start`). Both use a real spawned OS process (never mocked) with a
/// temp-dir storer standing in for Agent Control's real dynamic data dir, via the test-only
/// [`CommandOSNotStarted::with_process_record_storer`].
///
/// What this deliberately does not cover: Agent Control's own process actually crashing and
/// a second, independent OS process restarting and adopting across that crash. That needs a
/// real spawned binary and (to be faithful to `KillMode=process`) a real systemd unit, which
/// this in-process test harness can't provide, see `scripts/poc-validate-crash-survival.sh`
/// for that level instead.
#[cfg(all(test, target_os = "linux"))]
mod adoption_poc {
    use super::*;
    use crate::sub_agent::on_host::command::executable_data::ExecutableData;
    use crate::sub_agent::on_host::command::logging::file_logger::SubAgentFileLoggingConfig;
    use crate::sub_agent::on_host::command::process_record::{
        FileProcessRecordStorer, ProcessRecord, ProcessRecordStorer,
    };
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;
    use std::process::Command as StdCommand;

    fn storer_at(dir: &std::path::Path) -> FileProcessRecordStorer<LocalFile, DirectoryManagerFs> {
        FileProcessRecordStorer::new(LocalFile, DirectoryManagerFs, dir.to_path_buf())
    }

    fn spawn_sleeper() -> Child {
        StdCommand::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to spawn `sleep`")
    }

    #[test]
    fn adoptable_process_recognizes_a_still_alive_recorded_process() {
        let dir = tempfile::tempdir().unwrap();
        let storer = storer_at(dir.path());
        let agent_id = AgentID::try_from("adopt-alive").unwrap();
        let mut sleeper = spawn_sleeper();
        let pid = sleeper.id();
        let marker = process_record::read_proc_start_time_marker(pid)
            .expect("should be able to read the freshly spawned process's start-time marker");
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid,
                    start_time_marker: marker,
                },
            )
            .unwrap();

        let adopted = adoptable_process(&agent_id, &storer);

        assert!(
            adopted.is_some(),
            "expected the still-alive pid to be adoptable"
        );
        assert_eq!(adopted.unwrap().pid, pid);

        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
    }

    #[test]
    fn adoptable_process_rejects_a_mismatched_start_time_marker() {
        let dir = tempfile::tempdir().unwrap();
        let storer = storer_at(dir.path());
        let agent_id = AgentID::try_from("adopt-reused").unwrap();
        let mut sleeper = spawn_sleeper();
        let pid = sleeper.id();
        let real_marker = process_record::read_proc_start_time_marker(pid).unwrap();
        // Deliberately wrong marker: simulates this pid having been reused by a different
        // process than the one the bookkeeping record was made for.
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid,
                    start_time_marker: real_marker.wrapping_add(1),
                },
            )
            .unwrap();

        let adopted = adoptable_process(&agent_id, &storer);

        assert!(
            adopted.is_none(),
            "a mismatched start-time marker must never be adopted, pid-reuse would go undetected"
        );

        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
    }

    #[test]
    fn adoptable_process_returns_none_once_the_pid_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let storer = storer_at(dir.path());
        let agent_id = AgentID::try_from("adopt-gone").unwrap();
        let mut sleeper = spawn_sleeper();
        let pid = sleeper.id();
        let marker = process_record::read_proc_start_time_marker(pid).unwrap();
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid,
                    start_time_marker: marker,
                },
            )
            .unwrap();
        sleeper.kill().unwrap();
        sleeper.wait().unwrap();

        assert!(adoptable_process(&agent_id, &storer).is_none());
    }

    #[test]
    fn record_spawned_process_persists_a_record_readable_by_adoptable_process() {
        let dir = tempfile::tempdir().unwrap();
        let storer = storer_at(dir.path());
        let agent_id = AgentID::try_from("record-then-adopt").unwrap();
        let mut sleeper = spawn_sleeper();
        let pid = sleeper.id();

        record_spawned_process(&agent_id, pid, &storer);

        // Round-trips through the exact function `start()` calls after a real spawn, then
        // through the exact function it calls before a real spawn, closing the loop.
        let adopted = adoptable_process(&agent_id, &storer);
        assert_eq!(adopted.map(|a| a.pid), Some(pid));

        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
    }

    /// Drives the real call site, not just the helper functions: builds a
    /// [`CommandOSNotStarted`] for an `agent_id` that already has a live, matching
    /// bookkeeping record and confirms `start()` adopts it, `ManagedProcess::Adopted`,
    /// same pid, instead of spawning a duplicate.
    #[test]
    fn start_adopts_instead_of_spawning_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let storer: Arc<dyn ProcessRecordStorer + Send + Sync> = Arc::new(storer_at(dir.path()));
        let agent_id = AgentID::try_from("start-adopts").unwrap();
        let mut pre_existing = spawn_sleeper();
        let pid = pre_existing.id();
        let marker = process_record::read_proc_start_time_marker(pid).unwrap();
        storer
            .set(
                &agent_id,
                &ProcessRecord {
                    pid,
                    start_time_marker: marker,
                },
            )
            .unwrap();

        // The executable here is deliberately something that would be trivially
        // distinguishable from `pre_existing` if `start()` actually spawned it: if adoption
        // didn't kick in, `get_pid()` below would belong to this `true` invocation instead.
        let executable_data = ExecutableData::new("adopt-test".to_owned(), "true".to_owned());
        let command = CommandOSNotStarted::new(
            agent_id,
            &executable_data,
            SubAgentFileLoggingConfig::default(),
        )
        .with_process_record_storer(storer);

        let started = command
            .start()
            .expect("start() should succeed via adoption");

        assert_eq!(
            started.get_pid(),
            pid,
            "expected start() to adopt the pre-existing process instead of spawning `true`"
        );

        pre_existing.kill().unwrap();
        pre_existing.wait().unwrap();
    }
}
