use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Error, ErrorKind},
    marker::PhantomData,
    process::{Child, Command, Stdio},
    sync::mpsc::Sender,
};

use super::{
    CommandError, CommandExecutor, CommandHandle, CommandRunner, OutputEvent, OutputStreamer,
};

pub struct Unstarted;
pub struct Started;

pub struct ProcessRunner<State = Unstarted> {
    cmd: Option<Command>,
    process: Option<Child>,

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
        }
    }
}

impl CommandExecutor for ProcessRunner {
    type Error = CommandError;
    type Process = ProcessRunner<Started>;
    fn start(self) -> Result<Self::Process, Self::Error> {
        Ok(ProcessRunner {
            cmd: None,
            state: PhantomData,
            process: Some(self.cmd.ok_or(CommandError::CommandNotFound)?.spawn()?),
        })
    }
}

impl CommandHandle for ProcessRunner<Started> {
    type Error = CommandError;
    fn stop(self) -> Result<(), Self::Error> {
        Ok(self
            .process
            .ok_or(CommandError::ProcessNotStarted)?
            .kill()?)
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

impl OutputStreamer for ProcessRunner<Started> {
    type Error = CommandError;
    type Handle = ProcessRunner<Started>;

    fn stream(mut self, snd: Sender<OutputEvent>) -> Result<Self::Handle, Self::Error> {
        fn build_err(s: &str) -> CommandError {
            CommandError::IOError(Error::new(ErrorKind::Other, s))
        }
        let c = self
            .process
            .as_mut()
            .ok_or(build_err("Process not started"))?;

        let stdout = c.stdout.take().ok_or(build_err("stdout not piped"))?;
        let stderr = c.stderr.take().ok_or(build_err("stderr not piped"))?;
        let stdout = BufReader::new(stdout);
        let stderr = BufReader::new(stderr);

        // Send output to the channel
        std::thread::spawn(move || {
            let mut out = stdout.lines();
            let mut err = stderr.lines();
            let (mut out_done, mut err_done) = (false, false);
            loop {
                match out.next() {
                    Some(line) => snd.send(OutputEvent::Stdout(line.unwrap())).unwrap(),
                    None => out_done = true,
                }
                match err.next() {
                    Some(line) => snd.send(OutputEvent::Stderr(line.unwrap())).unwrap(),
                    None => err_done = true,
                }

                if out_done && err_done {
                    break;
                }
            }
        });

        Ok(self)
    }
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
    use crate::command::{CommandExecutor, CommandHandle, OutputStreamer};

    use super::OutputEvent;

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExector = bool;
    pub struct MockedCommandHandler;

    impl super::CommandExecutor for MockedCommandExector {
        type Error = CommandError;
        type Process = MockedCommandHandler;
        fn start(self) -> Result<Self::Process, Self::Error> {
            if self == true {
                Err(CommandError::ProcessError(ExitStatus::from_raw(1)))
            } else {
                Ok(MockedCommandHandler {})
            }
        }
    }

    impl CommandHandle for MockedCommandHandler {
        type Error = CommandError;
        fn stop(self) -> Result<(), CommandError> {
            Ok(())
        }
    }

    #[test]
    fn start_stop() {
        let cmds: Vec<MockedCommandExector> = vec![true, false, true, true, false];

        assert_eq!(
            cmds.iter()
                .map(|cmd| cmd.start())
                .filter(Result::is_ok)
                .count(),
            2
        )
    }

    impl OutputStreamer for MockedCommandHandler {
        type Error = CommandError;
        type Handle = MockedCommandHandler;

        fn stream(self, snd: Sender<OutputEvent>) -> Result<Self, Self::Error> {
            let esnd = snd.clone();
            (0..9).for_each(|i| {
                snd.send(OutputEvent::Stdout(format!("This is line {}", i)))
                    .unwrap()
            });
            (0..9).for_each(|i| {
                esnd.send(OutputEvent::Stderr(format!("This is error {}", i)))
                    .unwrap()
            });

            Ok(self)
        }
    }

    #[test]
    fn stream() {
        let cmd = MockedCommandHandler {};
        let (tx, rx) = std::sync::mpsc::channel();

        let cmd = cmd.stream(tx).unwrap();

        let mut stdout_expected = Vec::new();
        let mut stderr_expected = Vec::new();
        // Populate expected results in a similar way as the mocked streamer
        (0..9).for_each(|i| stdout_expected.push(format!("This is line {}", i)));
        (0..9).for_each(|i| stderr_expected.push(format!("This is error {}", i)));

        let mut stdout_result = Vec::new();
        let mut stderr_result = Vec::new();
        // Receive actual data from streamer
        rx.iter().for_each(|event| match event {
            OutputEvent::Stdout(line) => stdout_result.push(line),
            OutputEvent::Stderr(line) => stderr_result.push(line),
        });

        assert_eq!(stdout_expected, stdout_result);
        assert_eq!(stderr_expected, stderr_result);
        assert!(cmd.stop().is_ok());
    }
}
