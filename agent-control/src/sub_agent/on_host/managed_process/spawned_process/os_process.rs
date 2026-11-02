//! OS-level command wrapper with not-started/started states, output redirection, and shutdown handling.

use super::executable_data::ExecutableData;
use super::{error::CommandError, log_files::open_std_log_files};
use crate::agent_control::agent_id::AgentID;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
#[cfg(target_family = "unix")]
use std::time::Duration;

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
/// A configured but not-yet-spawned OS process.
pub struct OsProcessConfig {
    cmd: Command,
    agent_id: AgentID,
    log_dir: Option<PathBuf>,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

/// A spawned OS process.
pub struct OsProcess {
    agent_id: AgentID,
    process: Child,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started OS Process
////////////////////////////////////////////////////////////////////////////////////
impl OsProcessConfig {
    /// Creates a not-started process config for the given executable. Its stdout/stderr are written
    /// to log files in `log_dir` when provided, and discarded otherwise.
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
            #[cfg(target_family = "unix")]
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    /// Spawns the process with its stdout/stderr set up.
    pub fn start(mut self) -> Result<OsProcess, CommandError> {
        let (stdout, stderr) = self.std_streams()?;
        let child = self.cmd.stdout(stdout).stderr(stderr).spawn()?;
        Ok(OsProcess {
            agent_id: self.agent_id,
            process: child,
            #[cfg(target_family = "unix")]
            shutdown_timeout: self.shutdown_timeout,
        })
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
// Started OS Process
////////////////////////////////////////////////////////////////////////////////////

impl OsProcess {
    /// Returns the process id of the running process.
    pub fn get_pid(&self) -> u32 {
        self.process.id()
    }

    /// Returns the agent this process belongs to.
    pub(crate) fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    /// Returns how long to wait after a graceful shutdown before forcing a kill.
    #[cfg(target_family = "unix")]
    pub(crate) fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }

    /// Creates a Job Object and assigns this process to it.
    #[cfg(target_family = "windows")]
    pub(crate) fn create_job_object(
        &self,
    ) -> Result<crate::utils::job_object::JobObject, super::error::CommandError> {
        let job = crate::utils::job_object::JobObject::new()?;
        job.assign_process(&self.process)?;
        Ok(job)
    }
}
