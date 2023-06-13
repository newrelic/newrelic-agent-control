use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    marker::PhantomData,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{SendError, Sender},
    thread,
};

use super::{
    stream::{Event, Metadata},
    CommandError, CommandExecutor, CommandHandle, CommandRunner, EventStreamer, OutputEvent,
};

pub struct Unstarted;
pub struct Started;

pub struct ProcessRunner<State = Unstarted> {
    cmd: Option<Command>,
    process: Option<Child>,
    //
    metadata: Metadata,

    state: PhantomData<State>,
}

impl ProcessRunner {
    pub fn new<I, S>(binary_path: S, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(binary_path);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            cmd: Some(command),
            state: PhantomData,
            process: None,
            metadata: Metadata::default(),
        }
    }

    // TODO: move to builder?
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
}

impl CommandExecutor for ProcessRunner<Unstarted> {
    type Error = CommandError;
    type Process = ProcessRunner<Started>;
    fn start(self) -> Result<Self::Process, Self::Error> {
        Ok(ProcessRunner {
            cmd: None,
            state: PhantomData,
            process: Some(self.cmd.ok_or(CommandError::CommandNotFound)?.spawn()?),
            metadata: self.metadata,
        })
    }
}

impl CommandHandle for ProcessRunner<Started> {
    type Error = CommandError;

    fn wait(self) -> Result<ExitStatus, Self::Error> {
        self.process
            .ok_or(CommandError::ProcessNotStarted)?
            .wait()
            .map_err(CommandError::from)
    }

    fn get_pid(&self) -> u32 {
        // process should always be Some here
        self.process.as_ref().unwrap().id()
    }
}

impl CommandRunner for ProcessRunner {
    type Error = CommandError;
    fn run(self) -> Result<std::process::ExitStatus, Self::Error> {
        Ok(self
            .cmd
            .ok_or(CommandError::CommandNotFound)?
            .spawn()?
            .wait()?)
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
        let c = self
            .process
            .as_mut()
            .ok_or(CommandError::ProcessNotStarted)?;

        let stdout = c
            .stdout
            .take()
            .ok_or(CommandError::StreamPipeError("stdout".to_string()))?;
        let stderr = c
            .stderr
            .take()
            .ok_or(CommandError::StreamPipeError("stderr".to_string()))?;

        let fields_out: Metadata = Metadata::from(&self);
        let fields_err: Metadata = fields_out.clone();

        let out_snd = snd;
        let err_snd = out_snd.clone();

        read_stream(stdout, move |line| {
            out_snd.send(Event::new(OutputEvent::Stdout(line), fields_out.clone()))
        });
        read_stream(stderr, move |line| {
            err_snd.send(Event::new(OutputEvent::Stderr(line), fields_err.clone()))
        });

        Ok(self)
    }
}

fn read_stream<R, F>(stream: R, func: F)
where
    R: Read + Send + 'static,
    F: Fn(String) -> Result<(), SendError<Event>> + Send + 'static,
{
    let out = BufReader::new(stream).lines();
    thread::spawn(move || -> Result<(), CommandError> {
        for line in out {
            func(line?)?
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    #[cfg(target_family = "unix")]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(target_family = "windows")]
    use std::os::windows::process::ExitStatusExt;

    use std::process::ExitStatus;
    use std::sync::mpsc::Sender;

    use crate::command::error::CommandError;
    use crate::command::stream::{Event, Metadata};
    use crate::command::{CommandExecutor, CommandHandle, EventStreamer};

    use super::OutputEvent;

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExecutor = bool;
    pub struct MockedCommandHandler;

    impl super::CommandExecutor for MockedCommandExecutor {
        type Error = CommandError;
        type Process = MockedCommandHandler;
        fn start(self) -> Result<Self::Process, Self::Error> {
            if self {
                Err(CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler {})
            }
        }
    }

    impl CommandHandle for MockedCommandHandler {
        type Error = CommandError;
        fn wait(self) -> Result<ExitStatus, Self::Error> {
            Ok(ExitStatus::from_raw(0))
        }

        fn get_pid(&self) -> u32 {
            0
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
                OutputEvent::Stdout(line) => stdout_result.push(line),
                OutputEvent::Stderr(line) => stderr_result.push(line),
            }
        });

        assert_eq!(stdout_expected, stdout_result);
        assert_eq!(stderr_expected, stderr_result);
    }
}
