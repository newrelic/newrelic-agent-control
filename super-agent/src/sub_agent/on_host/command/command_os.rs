use std::{
    ffi::OsStr,
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
            state: NotStarted { cmd, logs_to_file },
        }
    }
}

impl NotStartedCommand for CommandOS<NotStarted> {
    type StartedCommand = CommandOS<Started>;
    fn start(mut self) -> Result<CommandOS<Started>, CommandError> {
        let agent_id = self.agent_id;
        let loggers = self.state.logs_to_file.then(|| {
            FileSystemLoggers::new(
                file_logger(&agent_id, STDOUT_LOG_PREFIX),
                file_logger(&agent_id, STDERR_LOG_PREFIX),
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
fn file_logger(agent_id: &AgentID, prefix: &str) -> FileLogger {
    FileAppender::new(agent_id, prefix).into()
}

impl CommandOS<NotStarted> {
    pub fn start_with_loggers(
        mut self,
        out: FileLogger,
        err: FileLogger,
    ) -> Result<CommandOS<Started>, CommandError> {
        let loggers = self
            .state
            .logs_to_file
            .then(|| FileSystemLoggers::new(out, err));
        Ok(CommandOS {
            agent_id: self.agent_id,
            state: Started {
                process: self.state.cmd.spawn()?,
                loggers,
            },
        })
    }
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

#[cfg(test)]
mod tests {
    use std::io;
    #[cfg(target_family = "unix")]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(target_family = "windows")]
    use std::os::windows::process::ExitStatusExt;
    use std::sync::{Arc, Mutex};

    use super::{CommandError, NotStartedCommand, StartedCommand};
    use std::{io::Write, process::ExitStatus};

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExecutor = bool;

    #[derive(Default)]
    pub struct MockedCommandHandler;

    impl NotStartedCommand for MockedCommandExecutor {
        type StartedCommand = MockedCommandHandler;

        fn start(self) -> Result<Self::StartedCommand, CommandError> {
            if self {
                Err(CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler)
            }
        }
    }

    impl StartedCommand for MockedCommandHandler {
        type StartedCommand = MockedCommandHandler;
        fn wait(self) -> Result<ExitStatus, CommandError> {
            Ok(ExitStatus::from_raw(0))
        }

        fn get_pid(&self) -> u32 {
            0
        }

        fn stream(self) -> Result<Self::StartedCommand, CommandError> {
            Ok(self)
        }
    }

    // Testing the file logging
    struct FileWriterMock(Arc<Mutex<Vec<String>>>);

    impl Write for FileWriterMock {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            // Trimming the string in this mock as line breaks are for files
            let s = String::from_utf8_lossy(buf).trim().to_string();
            self.0.lock().unwrap().push(s);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn start_stop() {
        let cmds: Vec<MockedCommandExecutor> = vec![true, false, true, true, false];

        assert_eq!(
            cmds.iter()
                .map(|cmd| cmd.start())
                .filter(Result::is_ok)
                .count(),
            2
        )
    }
}
