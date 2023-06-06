use std::{
    ffi::OsStr,
    io::{BufRead, BufReader},
    marker::PhantomData,
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::mpsc::Sender,
};

use super::{
    CommandError, CommandExecutor, CommandHandle, CommandRunner, OutputEvent, OutputStreamer,
};

use log::error;

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

impl CommandExecutor for ProcessRunner<Unstarted> {
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

impl OutputStreamer for ProcessRunner<Started> {
    type Error = CommandError;
    type Handle = ProcessRunner<Started>;

    fn stream(mut self, snd: Sender<OutputEvent>) -> Result<Self::Handle, Self::Error> {
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

        // Send output to the channel
        std::thread::spawn(move || {
            process_output_events(stdout, stderr, snd).map_err(|e| error!("stream error: {}", e))
        });

        Ok(self)
    }
}

fn process_output_events(
    stdout: ChildStdout,
    stderr: ChildStderr,
    snd: Sender<OutputEvent>,
) -> Result<(), CommandError> {
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let (mut out_done, mut err_done) = (false, false);

    loop {
        if let (false, Some(l)) = (out_done, out.next()) {
            snd.send(OutputEvent::Stdout(l?))?;
        } else {
            out_done = true;
        }

        if let (false, Some(l)) = (err_done, err.next()) {
            snd.send(OutputEvent::Stderr(l?))?;
        } else {
            err_done = true;
        }

        if out_done && err_done {
            break;
        }
    }
    Ok(())
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

        cmd.stream(tx).unwrap();

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
    }
}
