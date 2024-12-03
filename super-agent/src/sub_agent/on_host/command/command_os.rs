use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::super_agent::{
    config::AgentID,
    defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX},
};
use std::{
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
};

use super::{
    command::CommandError,
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
}
pub struct CommandOSStarted {
    agent_id: AgentID,
    process: Child,
    loggers: Option<FileSystemLoggers>,
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
        })
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started Command OS
////////////////////////////////////////////////////////////////////////////////////

impl CommandOSStarted {
    pub(crate) fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.process.wait().map_err(CommandError::from)
    }

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
            stdout_loggers.push(Logger::File(out, self.agent_id.clone()));
            stderr_loggers.push(Logger::File(err, self.agent_id.clone()));
        };

        // Read stdout and send to the channel
        logging::thread::spawn_logger(stdout, stdout_loggers);

        // Read stderr and send to the channel
        logging::thread::spawn_logger(stderr, stderr_loggers);

        Ok(self)
    }
}

/// Creates a new file logger for this agent id and file prefix
fn file_logger(agent_id: &AgentID, path: PathBuf, prefix: &str) -> FileLogger {
    FileAppender::new(agent_id, path, prefix).into()
}
