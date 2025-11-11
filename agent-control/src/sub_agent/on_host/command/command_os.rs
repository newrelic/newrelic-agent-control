use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX};
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use std::time::Duration;
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
        Ok(CommandOSStarted {
            agent_id,
            process: self.cmd.spawn()?,
            loggers,
            shutdown_timeout: self.shutdown_timeout,
        })
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
}

#[cfg(target_family = "unix")]
mod unix {
    use crate::sub_agent::on_host::command::{command_os::CommandOSStarted, error::CommandError};

    use std::time::Duration;
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    impl CommandOSStarted {
        pub fn shutdown(&mut self) -> Result<(), CommandError> {
            let pid = self.get_pid() as i32;

            use nix::{sys::signal, unistd::Pid};
            let graceful_shutdown_result = signal::kill(Pid::from_raw(pid), signal::SIGTERM)
                .map_err(|err| CommandError::NixError(err.to_string()));

            if graceful_shutdown_result.is_err()
                || self.is_running_after_timeout(self.shutdown_timeout)
            {
                self.process.kill().map_err(CommandError::from)?;
            }
            Ok(())
        }

        fn is_running_after_timeout(&mut self, timeout: Duration) -> bool {
            let deadline = std::time::Instant::now() + timeout;

            while std::time::Instant::now() < deadline {
                if self.is_running() {
                    std::thread::sleep(POLL_INTERVAL);
                } else {
                    return false;
                }
            }

            true
        }
    }
}

//TODO Properly design unix/windows shutdown when Windows support is added
#[cfg(target_family = "windows")]
mod windows {
    use crate::sub_agent::on_host::command::{command_os::CommandOSStarted, error::CommandError};

    impl CommandOSStarted {
        pub fn shutdown(&mut self) -> Result<(), CommandError> {
            self.process.kill().map_err(CommandError::from)
        }
    }
}

/// Creates a new file logger for this agent id and file prefix
fn file_logger(agent_id: &AgentID, path: PathBuf, prefix: &str) -> FileLogger {
    FileAppender::new(agent_id, path, prefix).into()
}
