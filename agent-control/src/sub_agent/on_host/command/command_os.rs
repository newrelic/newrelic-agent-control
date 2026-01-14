use tracing::warn;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX};
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use std::time::{Duration, Instant};
use std::{
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
};

use super::{
    error::CommandError,
    logging::{
        self,
        file_logger::{FileAppender, FileLogger, FileSystemLoggers},
        logger::Logger,
    },
};
#[cfg(target_family = "windows")]
use crate::sub_agent::on_host::command::job_object::JobObject;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
pub struct CommandOSNotStarted {
    cmd: Command,
    agent_id: AgentID,
    logs_to_file: bool,
    logging_path: PathBuf,
    shutdown_timeout: Duration,
}
pub struct CommandOSStarted {
    agent_id: AgentID,
    process: Child,
    loggers: Option<FileSystemLoggers>,
    shutdown_timeout: Duration,

    #[cfg(target_family = "windows")]
    job_object: Option<JobObject>,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
impl CommandOSNotStarted {
    pub fn new(
        agent_id: AgentID,
        executable_data: &ExecutableData,
        logs_to_file: bool,
        logging_path: PathBuf,
    ) -> Self {
        let mut cmd = Command::new(&executable_data.bin);
        cmd.args(&executable_data.args)
            .envs(&executable_data.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            agent_id,
            cmd,
            logs_to_file,
            logging_path,
            shutdown_timeout: executable_data.shutdown_timeout,
        }
    }

    pub fn start(mut self) -> Result<CommandOSStarted, CommandError> {
        let agent_id = self.agent_id;
        let loggers = self.logs_to_file.then(|| {
            FileSystemLoggers::new(
                file_logger(&agent_id, self.logging_path.clone(), STDOUT_LOG_PREFIX),
                file_logger(&agent_id, self.logging_path, STDERR_LOG_PREFIX),
            )
        });
        let child = self.cmd.spawn()?;

        #[cfg(target_family = "unix")]
        {
            Ok(CommandOSStarted {
                agent_id,
                process: child,
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
                agent_id,
                process: child,
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
    pub fn get_pid(&self) -> u32 {
        self.process.id()
    }

    pub fn is_running(&mut self) -> bool {
        self.process.try_wait().is_ok_and(|v| v.is_none())
    }

    pub(crate) fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.process.wait().map_err(CommandError::from)
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
            stdout_loggers.push(Logger::File(Box::new(out), self.agent_id.clone()));
            stderr_loggers.push(Logger::File(Box::new(err), self.agent_id.clone()));
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

/// Creates a new file logger for this agent id and file prefix
fn file_logger(agent_id: &AgentID, path: PathBuf, prefix: &str) -> FileLogger {
    FileAppender::new(agent_id, path, prefix).into()
}
