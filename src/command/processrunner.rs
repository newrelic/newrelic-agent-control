use std::{
    ffi::OsStr,
    io::{BufReader, Error, ErrorKind},
    marker::PhantomData,
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
};

use super::{CommandError, CommandExecutor, CommandHandle, CommandRunner, OutputStreamer};

type OutputStream<Out, Err> = (BufReader<Out>, BufReader<Err>);

pub struct Unstarted;
pub struct Started;

pub struct ProcessRunner<State = Unstarted> {
    cmd: Option<Command>,
    process: Option<Child>,

    stream: Option<OutputStream<ChildStdout, ChildStderr>>,
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
            stream: None,
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

    fn stream(mut self) -> Result<Self::Handle, Self::Error> {
        fn build_err(s: &str) -> CommandError {
            CommandError::IOError(Error::new(ErrorKind::Other, s))
        }
        let c = self
            .process
            .as_mut()
            .ok_or(build_err("Process not started"))?;
        let stdout = c.stdout.take().ok_or(build_err("stdout not piped"))?;
        let stderr = c.stderr.take().ok_or(build_err("stderr not piped"))?;

        let stdout_r = BufReader::new(stdout);
        let stderr_r = BufReader::new(stderr);

        self.stream = Some((stdout_r, stderr_r));

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

    use crate::command::error::CommandError;
    use crate::command::{CommandExecutor, CommandHandle, OutputStreamer};

    use super::OutputStream;

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

    pub struct MockedCommandHandlerWithStream {
        stream: Option<OutputStream<Cursor<&'static str>, Cursor<&'static str>>>,
    }

    impl CommandHandle for MockedCommandHandlerWithStream {
        type Error = CommandError;
        fn stop(self) -> Result<(), CommandError> {
            Ok(())
        }
    }

    impl OutputStreamer for MockedCommandHandlerWithStream {
        type Error = CommandError;
        type Handle = MockedCommandHandlerWithStream;
        fn stream(mut self) -> Result<Self::Handle, Self::Error> {
            let stdout_r = Cursor::new("This is the first line\nThis is the second line\n");
            let stderr_r = Cursor::new("This is the first error\nThis is the second error\n");

            self.stream = Some((BufReader::new(stdout_r), BufReader::new(stderr_r)));
            Ok(self)
        }
    }

    #[test]
    fn stream() {
        let cmd = MockedCommandHandlerWithStream { stream: None };
        let cmd = cmd.stream().unwrap();
        let (stdout, stderr) = cmd.stream.unwrap();

        let mut stdout_result = Vec::new();
        let mut stderr_result = Vec::new();

        stdout.lines().for_each(|line| {
            stdout_result.push(line.unwrap());
        });

        stderr.lines().for_each(|line| {
            stderr_result.push(line.unwrap());
        });

        assert_eq!(
            stdout_result,
            vec!["This is the first line", "This is the second line"]
        );
        assert_eq!(
            stderr_result,
            vec!["This is the first error", "This is the second error"]
        );
    }
}
