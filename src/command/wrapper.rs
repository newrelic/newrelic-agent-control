use std::{
    ffi::OsStr,
    marker::PhantomData,
    process::{Child, Command},
};

use crate::command::ipc::{notify as IPCNotify, Error as IPCError};
use super::{CommandError, CommandExecutor, CommandHandle, CommandNotifier, CommandRunner, Message};

pub struct Unstarted;
pub struct Started;

pub struct ProcessRunner<State = Unstarted> {
    cmd: Option<Command>,
    process: Option<Child>,
    pid: u32,

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
            pid: 0,
        }
    }
}

impl CommandExecutor for ProcessRunner {
    type Error = CommandError;
    type Process = ProcessRunner<Started>;
    fn start(self) -> Result<Self::Process, Self::Error> {
        let process = self.cmd.unwrap().spawn()?;
        let pid = process.id();

        Ok(ProcessRunner {
            cmd: None,
            state: PhantomData,
            process: Some(process),
            pid,
        })
    }
}

impl CommandHandle for ProcessRunner<Started> {
    type Error = CommandError;
    fn stop(self) -> Result<(), Self::Error> {
        Ok(self.process.unwrap().kill()?)
    }
}

impl CommandNotifier for ProcessRunner<Started>{
    type Error = IPCError;
    fn notify(&self, msg:Message) -> Result<(), Self::Error> {
        IPCNotify(self.pid, msg)
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
    use crate::command::{CommandExecutor, CommandHandle, CommandNotifier};
    use crate::command::ipc::Message;

    // MockedCommandExector returns an error on start if fail is true
    // It can be used to mock process spawn
    type MockedCommandExector = bool;
    pub struct MockedCommandHandler;

    impl CommandExecutor for MockedCommandExector {
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

    impl CommandNotifier for MockedCommandHandler {
        type Error = CommandError;
        fn notify(&self, _: Message) -> Result<(), Self::Error> {
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
