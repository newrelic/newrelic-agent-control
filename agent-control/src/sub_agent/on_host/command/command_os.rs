//! OS-level command wrapper with not-started/started states, output redirection, and shutdown handling.

use tracing::warn;

use super::{error::CommandError, log_files::open_std_log_files};
use crate::agent_control::agent_id::AgentID;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
#[cfg(target_family = "windows")]
use crate::utils::job_object::JobObject;
use std::path::PathBuf;
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
    log_dir: Option<PathBuf>,
    shutdown_timeout: Duration,
}
/// A spawned OS command process with (on Windows) its job object.
pub struct CommandOSStarted {
    agent_id: AgentID,
    process: Child,
    shutdown_timeout: Duration,

    #[cfg(target_family = "windows")]
    job_object: Option<JobObject>,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
impl CommandOSNotStarted {
    /// Creates a not-started command for the given executable. Its stdout/stderr are written to log
    /// files in `log_dir` when provided, and discarded otherwise.
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        log_dir: Option<PathBuf>,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args).envs(&executable_data.env);

        Self {
            agent_id,
            cmd,
            log_dir,
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    /// Spawns the process with its stdout/stderr set up and (on Windows) a job object.
    pub fn start(mut self) -> Result<CommandOSStarted, CommandError> {
        let (stdout, stderr) = self.std_streams()?;
        let child = self.cmd.stdout(stdout).stderr(stderr).spawn()?;

        #[cfg(target_family = "unix")]
        {
            Ok(CommandOSStarted {
                agent_id: self.agent_id,
                process: child,
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
                process: child,
                job_object: Some(job_object),
                shutdown_timeout: self.shutdown_timeout,
            })
        }
    }

    /// Returns the process stdout/stderr: the log files in `log_dir` when set, discarded otherwise.
    /// The process owns its handles to them, so its output does not depend on Agent Control being
    /// alive.
    fn std_streams(&self) -> Result<(Stdio, Stdio), CommandError> {
        let Some(log_dir) = &self.log_dir else {
            return Ok((Stdio::null(), Stdio::null()));
        };
        let (stdout, stderr) = open_std_log_files(log_dir)?;
        Ok((stdout.into(), stderr.into()))
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
