//! OS-level command wrapper with not-started/started states, output streaming, and shutdown handling.

use super::executable_data::ExecutableData;
use super::logging::file_logger::SubAgentFileLoggingConfig;
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
    file_logging_config: SubAgentFileLoggingConfig,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

/// A spawned OS process with its loggers.
pub struct OsProcess {
    agent_id: AgentID,
    process: Child,
    loggers: Option<FileSystemLoggers>,
    #[cfg(target_family = "unix")]
    shutdown_timeout: Duration,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started OS Process
////////////////////////////////////////////////////////////////////////////////////
impl OsProcessConfig {
    /// Creates a not-started process config for the given executable, configuring stdio pipes and logging.
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
            #[cfg(target_family = "unix")]
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    /// Spawns the process, setting up file loggers and (on Windows) a job object.
    pub fn start(mut self) -> Result<OsProcess, CommandError> {
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
        Ok(OsProcess {
            agent_id: self.agent_id,
            process: child,
            loggers,
            #[cfg(target_family = "unix")]
            shutdown_timeout: self.shutdown_timeout,
        })
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

    pub(crate) fn stream(mut self) -> Result<Self, CommandError> {
        let stdout = self
            .process
            .stdout
            .take()
            .ok_or(CommandError::StreamPipeError("stdout".to_string()))?;

        let stderr = self
            .process
            .stderr
            .take()
            .ok_or(CommandError::StreamPipeError("stderr".to_string()))?;

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
