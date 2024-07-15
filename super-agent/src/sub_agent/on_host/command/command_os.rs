use std::{
    ffi::OsStr,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
};

use crate::super_agent::{
    config::AgentID,
    defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX},
};

use super::{
    command::{CommandError, NotStartedCommand, StartedCommand, SyncCommandRunner},
    logging::{
        self,
        file_logger::{FileAppender, FileLogger, FileSystemLoggers},
        logger::Logger,
    },
};

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started/Sync Command
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted {
    cmd: Command,
    logs_to_file: bool,
    logging_path: PathBuf,
}
pub struct Started {
    process: Child,
    loggers: Option<FileSystemLoggers>,
}

pub struct Sync {
    cmd: Command,
}

////////////////////////////////////////////////////////////////////////////////////
// Command OS
////////////////////////////////////////////////////////////////////////////////////
pub struct CommandOS<S> {
    agent_id: AgentID,
    state: S,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
impl CommandOS<NotStarted> {
    pub fn new<I, E, K, S>(
        agent_id: AgentID,
        binary_path: S,
        args: I,
        envs: E,
        logs_to_file: bool,
        logging_path: PathBuf,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        E: IntoIterator<Item = (K, S)>,
        K: AsRef<OsStr>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(binary_path);
        cmd.args(args)
            .envs(envs)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            agent_id,
            state: NotStarted {
                cmd,
                logs_to_file,
                logging_path,
            },
        }
    }
}

impl NotStartedCommand for CommandOS<NotStarted> {
    type StartedCommand = CommandOS<Started>;
    fn start(mut self) -> Result<CommandOS<Started>, CommandError> {
        let agent_id = self.agent_id;
        let loggers = self.state.logs_to_file.then(|| {
            FileSystemLoggers::new(
                file_logger(
                    &agent_id,
                    self.state.logging_path.clone(),
                    STDOUT_LOG_PREFIX(),
                ),
                file_logger(&agent_id, self.state.logging_path, STDERR_LOG_PREFIX()),
            )
        });
        Ok(CommandOS {
            agent_id,
            state: Started {
                process: self.state.cmd.spawn()?,
                loggers,
            },
        })
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started Command OS
////////////////////////////////////////////////////////////////////////////////////

impl StartedCommand for CommandOS<Started> {
    type StartedCommand = CommandOS<Started>;

    fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.state.process.wait().map_err(CommandError::from)
    }

    fn get_pid(&self) -> u32 {
        self.state.process.id()
    }

    fn stream(mut self) -> Result<Self, CommandError> {
        let stdout = self
            .state
            .process
            .stdout
            .take()
            .ok_or(CommandError::StreamPipeError("stdout".to_string()))?;

        let stderr = self
            .state
            .process
            .stderr
            .take()
            .ok_or(CommandError::StreamPipeError("stderr".to_string()))?;

        let mut stdout_loggers = vec![Logger::Stdout];
        let mut stderr_loggers = vec![Logger::Stderr];

        if let Some(l) = self.state.loggers.take() {
            let (out, err) = l.into_loggers();
            stdout_loggers.push(out.into());
            stderr_loggers.push(err.into());
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

////////////////////////////////////////////////////////////////////////////////////
// Sync/Blocking Command OS
////////////////////////////////////////////////////////////////////////////////////
impl SyncCommandRunner for CommandOS<Sync> {
    fn run(mut self) -> Result<ExitStatus, CommandError> {
        Ok(self.state.cmd.spawn()?.wait()?)
    }
}

impl CommandOS<Sync> {
    pub fn new<I, E, K, S>(agent_id: AgentID, binary_path: S, args: I, envs: E) -> Self
    where
        I: IntoIterator<Item = S>,
        E: IntoIterator<Item = (K, S)>,
        K: AsRef<OsStr>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(binary_path);
        cmd.args(args)
            .envs(envs)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            agent_id,
            state: Sync { cmd },
        }
    }
}
