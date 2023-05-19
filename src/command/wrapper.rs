use std::{
    ffi::OsStr,
    marker::PhantomData,
    process::{Child, Command},
};

use super::{CommandError, CommandExecutor, CommandHandle, CommandRunner};

pub struct Unstarted;
pub struct Started;

pub struct ProcessRunner<State = Unstarted> {
    cmd: Option<Command>,
    process: Option<Child>,

    state: PhantomData<State>,
}

impl ProcessRunner {
    pub fn new<I, S>(binary_path: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(binary_path);
        command.args(args);

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
            process: Some(self.cmd.unwrap().spawn()?),
        })
    }
}

impl CommandHandle for ProcessRunner<Started> {
    type Error = CommandError;
    fn stop(self) -> Result<(), Self::Error> {
        Ok(self.process.unwrap().kill()?)
    }
}

impl CommandRunner for ProcessRunner {
    type Error = CommandError;
    fn run(self) -> Result<std::process::ExitStatus, Self::Error> {
        Ok(self.cmd.unwrap().spawn()?.wait()?)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use crate::command::error::CommandError;
    use crate::command::{CommandExecutor, CommandHandle};

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
}
