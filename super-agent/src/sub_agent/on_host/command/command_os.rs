use std::{
    ffi::OsStr,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::Sender,
};

use crate::{
    sub_agent::logger::{AgentLog, Metadata},
    super_agent::{
        config::AgentID,
        defaults::{STDERR_LOG_PREFIX, STDOUT_LOG_PREFIX},
    },
};

use super::{
    command::{CommandError, NotStartedCommand, StartedCommand, SyncCommandRunner},
    logging::{
        self,
        file_logger::{FileAppender, FileLogger, FileSystemLoggers},
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
    metadata: Metadata,
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
            state: NotStarted { cmd, logs_to_file },
            metadata: Metadata::new(agent_id),
        }
    }

    // TODO: move to builder?
    pub fn with_metadata(self, metadata: Metadata) -> Self {
        Self { metadata, ..self }
    }

    // TODO: I assume this is the agent id?
    pub fn id(&self) -> String {
        self.metadata.get_agent_id().to_string()
    }
}

impl NotStartedCommand for CommandOS<NotStarted> {
    type StartedCommand = CommandOS<Started>;
    fn start(mut self) -> Result<CommandOS<Started>, CommandError> {
        let loggers = self.state.logs_to_file.then(|| {
            let agent_id = self.metadata.get_agent_id();
            FileSystemLoggers::new(
                file_logger(agent_id, STDOUT_LOG_PREFIX),
                file_logger(agent_id, STDERR_LOG_PREFIX),
            )
        });
        Ok(CommandOS {
            state: Started {
                process: self.state.cmd.spawn()?,
                loggers,
            },
            metadata: self.metadata,
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

    fn stream(mut self, snd: Sender<AgentLog>) -> Result<Self, CommandError> {
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

        let fields: Metadata = self.metadata.clone();

        let (out, err) = self.state.loggers.take().map_or(Default::default(), |l| {
            let (out, err) = l.into_loggers();
            (Some(out), Some(err))
        });

        // Read stdout and send to the channel
        logging::thread::spawn_logger(fields.clone(), snd.clone(), stdout.into(), out);

        // Read stderr and send to the channel
        logging::thread::spawn_logger(fields.clone(), snd.clone(), stderr.into(), err);

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
            state: Started {
                process: self.state.cmd.spawn()?,
                loggers,
            },
            metadata: self.metadata,
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
            state: Sync { cmd },
            metadata: Metadata::new(agent_id),
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

    use tracing::info;

    use crate::sub_agent::on_host::command::logging::file_logger::{
        FileAppender, FileSystemLoggers,
    };
    use crate::sub_agent::{logger::LogOutput, on_host::command::logging::file_logger::FileLogger};

    use super::{CommandError, NotStartedCommand, StartedCommand};
    use std::sync::mpsc::Sender;
    use std::{io::Write, process::ExitStatus};

    use super::{AgentLog, Metadata};

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExecutor = bool;

    #[derive(Default)]
    pub struct MockedCommandHandler {
        loggers: Option<FileSystemLoggers>,
    }

    impl NotStartedCommand for MockedCommandExecutor {
        type StartedCommand = MockedCommandHandler;

        fn start(self) -> Result<Self::StartedCommand, CommandError> {
            if self {
                Err(CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler::default())
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

        fn stream(mut self, snd: Sender<AgentLog>) -> Result<Self::StartedCommand, CommandError> {
            let metadata = Metadata::from(&self);
            let (out, err) = self.loggers.take().map_or(Default::default(), |l| {
                let (out, err) = l.into_loggers();
                (Some(out), Some(err))
            });

            let guard = out.map(FileLogger::set_file_logging);
            (0..9).for_each(|i| {
                let line = format!("This is line {}", i);
                info!(file_log_line = line);
                snd.send(AgentLog {
                    output: LogOutput::Stdout(line),
                    metadata: metadata.clone(),
                })
                .unwrap();
            });
            drop(guard);

            let guard = err.map(FileLogger::set_file_logging);
            (0..9).for_each(|i| {
                let line = format!("This is error {}", i);
                info!(file_log_line = line);
                snd.send(AgentLog {
                    output: LogOutput::Stderr(line),
                    metadata: metadata.clone(),
                })
                .unwrap()
            });
            drop(guard);

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

    impl From<&MockedCommandHandler> for Metadata {
        fn from(_value: &MockedCommandHandler) -> Self {
            Metadata::new("mocked-id".to_owned().try_into().unwrap())
        }
    }

    #[test]
    fn stream() {
        let cmd = MockedCommandHandler::default();
        let (tx, rx) = std::sync::mpsc::channel();

        cmd.stream(tx).unwrap();

        let mut stdout_expected = Vec::new();
        let mut stderr_expected = Vec::new();
        // Populate expected results in a similar way as the mocked streamer
        (0..9).for_each(|i| stdout_expected.push(format!("This is line {}", i)));
        (0..9).for_each(|i| stderr_expected.push(format!("This is error {}", i)));

        let mut stdout_result = Vec::new();
        let mut stderr_result = Vec::new();
        // Receive actual data from streamer
        rx.iter().for_each(|event| {
            assert_eq!(
                Metadata::new("mocked-id".to_owned().try_into().unwrap()),
                event.metadata
            );
            match event.output {
                LogOutput::Stdout(line) => stdout_result.push(line),
                LogOutput::Stderr(line) => stderr_result.push(line),
            }
        });

        assert_eq!(stdout_expected, stdout_result);
        assert_eq!(stderr_expected, stderr_result);
    }

    #[test]
    fn stream_file_logging() {
        // Inner writer mocks
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));

        // Writer mocks
        let stdout_writer = FileWriterMock(stdout.clone());
        let stderr_writer = FileWriterMock(stderr.clone());

        // Generate appenders for mocks
        let stdout_appender = FileAppender::from(stdout_writer);
        let stderr_appender = FileAppender::from(stderr_writer);

        // Get actual loggers
        let stdout_logger = FileLogger::from(stdout_appender);
        let stderr_logger = FileLogger::from(stderr_appender);

        let cmd = MockedCommandHandler {
            loggers: Some(FileSystemLoggers::new(stdout_logger, stderr_logger)),
        };

        let (tx, rx) = std::sync::mpsc::channel();

        // Stream with the mock loggers provided
        cmd.stream(tx).unwrap();

        // Expected contents
        let mut stdout_expected = Vec::new();
        let mut stderr_expected = Vec::new();
        // Populate expected results in a similar way as the mocked file_logger
        (0..9).for_each(|i| stdout_expected.push(format!("This is line {}", i)));
        (0..9).for_each(|i| stderr_expected.push(format!("This is error {}", i)));

        // Receive actual data from streamer
        rx.iter().for_each(|event| {
            // Receive the data over the channel but do nothing with this
            assert_eq!(
                Metadata::new("mocked-id".to_owned().try_into().unwrap()),
                event.metadata
            );
        });

        // Check the contents of the file logger
        let stdout = stdout.lock().unwrap();
        assert_eq!(stdout_expected, *stdout);
        drop(stdout);

        let stderr = stderr.lock().unwrap();
        assert_eq!(stderr_expected, *stderr);
        drop(stderr);
    }
}
