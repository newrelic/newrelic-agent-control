use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{SendError, Sender},
};

use crate::sub_agent::logger::{AgentLog, LogOutput, Metadata};

use super::command::{CommandError, NotStartedCommand, StartedCommand, SyncCommandRunner};
use tracing::error;

////////////////////////////////////////////////////////////////////////////////////
// Not Started Command OS
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedCommandOS {
    cmd: Command,
    metadata: Metadata,
}

impl NotStartedCommandOS {
    pub fn new<I, E, K, S>(binary_path: S, args: I, envs: E) -> Self
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
            cmd,
            metadata: Metadata::default(),
        }
    }

    // TODO: move to builder?
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn id(&self) -> String {
        unimplemented!("TODO")
    }
}

impl NotStartedCommand for NotStartedCommandOS {
    type StartedCommand = StartedCommandOS;
    fn start(mut self) -> Result<StartedCommandOS, CommandError> {
        Ok(StartedCommandOS {
            process: self.cmd.spawn()?,
            metadata: self.metadata,
        })
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started Command OS
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedCommandOS {
    process: Child,
    metadata: Metadata,
}

impl StartedCommand for StartedCommandOS {
    type StartedCommand = StartedCommandOS;

    fn wait(mut self) -> Result<ExitStatus, CommandError> {
        self.process.wait().map_err(CommandError::from)
    }

    fn get_pid(&self) -> u32 {
        self.process.id()
    }

    fn stream(mut self, snd: Sender<AgentLog>) -> Result<Self, CommandError> {
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

        let fields: Metadata = self.metadata.clone();

        // Read stdout and send to the channel
        std::thread::spawn({
            let fields = fields.clone();
            let snd = snd.clone();
            move || {
                process_events(stdout, |line| {
                    snd.send(AgentLog {
                        metadata: fields.clone(),
                        output: LogOutput::Stdout(line),
                    })
                })
                .map_err(|e| error!("stdout stream error: {}", e))
            }
        });

        // Read stderr and send to the channel
        std::thread::spawn(move || {
            process_events(stderr, |line| {
                snd.send(AgentLog {
                    output: LogOutput::Stderr(line),
                    metadata: fields.clone(),
                })
            })
            .map_err(|e| error!("stderr stream error: {}", e))
        });

        Ok(self)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Sync/Blocking Command OS
////////////////////////////////////////////////////////////////////////////////////
pub struct SyncCommandOS {
    cmd: Command,
}

impl SyncCommandOS {
    pub fn new<I, E, K, S>(binary_path: S, args: I, envs: E) -> Self
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

        Self { cmd }
    }
}

impl SyncCommandRunner for SyncCommandOS {
    fn run(mut self) -> Result<ExitStatus, CommandError> {
        Ok(self.cmd.spawn()?.wait()?)
    }
}

fn process_events<R, F>(stream: R, send: F) -> Result<(), CommandError>
where
    R: Read,
    F: Fn(String) -> Result<(), SendError<AgentLog>>,
{
    let out = BufReader::new(stream).lines();
    for line in out {
        send(line?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_family = "unix")]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(target_family = "windows")]
    use std::os::windows::process::ExitStatusExt;

    use super::{CommandError, NotStartedCommand, StartedCommand};
    use std::process::ExitStatus;
    use std::sync::mpsc::Sender;

    use super::{AgentLog, LogOutput, Metadata};

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExecutor = bool;

    pub struct MockedCommandHandler;

    impl NotStartedCommand for MockedCommandExecutor {
        type StartedCommand = MockedCommandHandler;

        fn start(self) -> Result<Self::StartedCommand, CommandError> {
            if self {
                Err(CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler {})
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

        fn stream(self, snd: Sender<AgentLog>) -> Result<Self::StartedCommand, CommandError> {
            (0..9).for_each(|i| {
                snd.send(AgentLog {
                    output: LogOutput::Stdout(format!("This is line {}", i)),
                    metadata: Metadata::from(&self),
                })
                .unwrap()
            });
            (0..9).for_each(|i| {
                snd.send(AgentLog {
                    output: LogOutput::Stderr(format!("This is error {}", i)),
                    metadata: Metadata::from(&self),
                })
                .unwrap()
            });

            Ok(self)
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
            Metadata::new("mocked")
        }
    }

    #[test]
    fn stream() {
        let cmd = MockedCommandHandler {};
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
            assert_eq!(Metadata::new("mocked"), event.metadata);
            match event.output {
                LogOutput::Stdout(line) => stdout_result.push(line),
                LogOutput::Stderr(line) => stderr_result.push(line),
            }
        });

        assert_eq!(stdout_expected, stdout_result);
        assert_eq!(stderr_expected, stderr_result);
    }
}
