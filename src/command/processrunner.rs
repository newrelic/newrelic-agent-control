use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{SendError, Sender},
};

use super::{
    stream::{Event, Metadata},
    CommandBuilder, CommandError, CommandExecutor, CommandHandle, CommandRunner, EventStreamer,
    OutputEvent,
};

use tracing::error;

pub struct Unstarted {
    cmd: Command,
}

pub struct Started {
    process: Child,
}

pub struct ProcessRunner<State = Unstarted> {
    metadata: Metadata,

    state: State,
}

pub struct ProcessRunnerBuilder {
    binary_path: String,
    args: Vec<String>,
}

impl CommandBuilder for ProcessRunnerBuilder {
    type OutputType = ProcessRunner;
    fn build(&self) -> Self::OutputType {
        ProcessRunner::new(self.binary_path.clone(), self.args.clone())
    }
}

impl ProcessRunnerBuilder {
    pub fn new(binary_path: String, args: Vec<String>) -> Self {
        Self { binary_path, args }
    }
}

impl ProcessRunner {
    pub fn new<I, S>(binary_path: S, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(binary_path);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

        Self {
            state: Unstarted { cmd },
            // TODO: create with binary path
            metadata: Metadata::default(),
        }
    }

    // TODO: rename to append_metadata (e.g supervisor ID)
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
}

impl CommandExecutor for ProcessRunner<Unstarted> {
    type Error = CommandError;
    type Process = ProcessRunner<Started>;
    fn start(mut self) -> Result<Self::Process, Self::Error> {
        let process = self.state.cmd.spawn()?;
        Ok(ProcessRunner {
            state: Started { process },
            metadata: self.metadata,
        })
    }
}

impl CommandHandle for ProcessRunner<Started> {
    type Error = CommandError;

    fn wait(mut self) -> Result<ExitStatus, Self::Error> {
        self.state.process.wait().map_err(CommandError::from)
    }

    fn get_pid(&self) -> u32 {
        self.state.process.id()
    }
}

impl CommandRunner for ProcessRunner {
    type Error = CommandError;
    fn run(mut self) -> Result<std::process::ExitStatus, Self::Error> {
        Ok(self.state.cmd.spawn()?.wait()?)
    }
}

impl From<&ProcessRunner<Started>> for Metadata {
    fn from(value: &ProcessRunner<Started>) -> Self {
        value.metadata.clone()
    }
}

impl EventStreamer for ProcessRunner<Started> {
    type Error = CommandError;
    type Handle = ProcessRunner<Started>;

    fn stream(mut self, snd: Sender<Event>) -> Result<Self::Handle, Self::Error> {
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

        let fields: Metadata = Metadata::from(&self);

        // Read stdout and send to the channel
        std::thread::spawn({
            let fields = fields.clone();
            let snd = snd.clone();
            move || {
                process_events(stdout, |line| {
                    snd.send(Event::new(OutputEvent::Stdout(line), fields.clone()))
                })
                .map_err(|e| error!("stdout stream error: {}", e))
            }
        });

        // Read stderr and send to the channel
        std::thread::spawn(move || {
            process_events(stderr, |line| {
                snd.send(Event::new(OutputEvent::Stderr(line), fields.clone()))
            })
            .map_err(|e| error!("stderr stream error: {}", e))
        });

        Ok(self)
    }
}

fn process_events<R, F>(stream: R, send: F) -> Result<(), CommandError>
where
    R: Read,
    F: Fn(String) -> Result<(), SendError<Event>>,
{
    let out = BufReader::new(stream).lines();
    for line in out {
        send(line?)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod sleep_process_builder {
    #[cfg(target_family = "unix")]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(target_family = "windows")]
    use std::os::windows::process::ExitStatusExt;

    use std::{process::ExitStatus, thread::sleep, time::Duration};

    use crate::command::{CommandBuilder, CommandExecutor, CommandHandle};

    pub(crate) struct MockedCommandExecutor(pub bool, pub Duration);
    pub struct MockedCommandHandler(pub Duration);

    impl CommandExecutor for MockedCommandExecutor {
        type Error = super::CommandError;
        type Process = MockedCommandHandler;
        fn start(self) -> Result<Self::Process, Self::Error> {
            if self.0 {
                Err(super::CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler(self.1))
            }
        }
    }

    impl CommandHandle for MockedCommandHandler {
        type Error = super::CommandError;
        fn wait(self) -> Result<ExitStatus, Self::Error> {
            sleep(self.0);
            Ok(ExitStatus::from_raw(0))
        }

        fn get_pid(&self) -> u32 {
            0
        }
    }

    pub(crate) struct MockedProcessBuilder {
        fail_on_start: bool,
        sleepy: Duration,
    }

    impl MockedProcessBuilder {
        fn new(fail_on_start: bool, sleepy: Duration) -> Self {
            Self {
                fail_on_start,
                sleepy,
            }
        }
    }

    impl CommandBuilder for MockedProcessBuilder {
        type OutputType = MockedCommandExecutor;
        fn build(&self) -> Self::OutputType {
            MockedCommandExecutor(self.fail_on_start, self.sleepy)
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use crate::command::error::CommandError;
    use crate::command::processrunner::sleep_process_builder::MockedCommandExecutor;
    use crate::command::stream::{Event, Metadata};
    use crate::command::{CommandExecutor, EventStreamer};

    use super::sleep_process_builder::MockedCommandHandler;
    use super::OutputEvent;

    #[test]
    fn start_stop() {
        let cmds: Vec<MockedCommandExecutor> = vec![
            MockedCommandExecutor(true, Duration::new(0, 0)),
            MockedCommandExecutor(false, Duration::new(0, 0)),
            MockedCommandExecutor(true, Duration::new(0, 0)),
            MockedCommandExecutor(true, Duration::new(0, 0)),
            MockedCommandExecutor(false, Duration::new(0, 0)),
        ];

        assert_eq!(
            cmds.into_iter()
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

    impl EventStreamer for MockedCommandHandler {
        type Error = CommandError;
        type Handle = MockedCommandHandler;

        fn stream(self, snd: Sender<Event>) -> Result<Self, Self::Error> {
            (0..9).for_each(|i| {
                snd.send(Event::new(
                    OutputEvent::Stdout(format!("This is line {}", i)),
                    Metadata::from(&self),
                ))
                .unwrap()
            });
            (0..9).for_each(|i| {
                snd.send(Event::new(
                    OutputEvent::Stderr(format!("This is error {}", i)),
                    Metadata::from(&self),
                ))
                .unwrap()
            });

            Ok(self)
        }
    }

    #[test]
    fn stream() {
        let cmd = MockedCommandHandler(Duration::new(0, 0));
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
                OutputEvent::Stdout(line) => stdout_result.push(line),
                OutputEvent::Stderr(line) => stderr_result.push(line),
            }
        });

        assert_eq!(stdout_expected, stdout_result);
        assert_eq!(stderr_expected, stderr_result);
    }
}
