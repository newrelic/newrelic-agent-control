//! OS-level command wrapper with not-started/started states, output streaming, and shutdown handling.

use tracing::warn;

use super::{
    error::CommandError,
    logging::{
        self,
        file_logger::{FileSystemLoggers, file_logger},
        logger::Logger,
    },
};
#[cfg(target_os = "linux")]
use super::process_record;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_FILE_NAME_SUFFIX, STDOUT_LOG_FILE_NAME_SUFFIX};
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::command::logging::file_logger::SubAgentFileLoggingConfig;
#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
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
/// (`Adopted`, not yet constructed anywhere; see `process_record`). Kept as a thin wrapper
/// mirroring `Child`'s API so `CommandOSStarted`'s own methods barely change either way.
enum ManagedProcess {
    Spawned(Child),
    #[allow(dead_code)] // constructed once discovery/adoption is wired in
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
        signal::kill(Pid::from_raw(self.pid as i32), signal::SIGKILL)
            .map_err(std::io::Error::from)
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
        }
    }

    /// Spawns the process, setting up file loggers and (on Windows) a job object.
    pub fn start(mut self) -> Result<CommandOSStarted, CommandError> {
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
