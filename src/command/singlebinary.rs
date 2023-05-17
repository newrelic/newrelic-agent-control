use std::{
    ffi::OsStr,
    process::{Child, Command as StandardCommand},
};

use super::{Command, CommandError, CommandExecutor, Process};

impl Command for StandardCommand {
    type Proc = Child;
    fn spawn(&mut self) -> std::io::Result<Child> {
        self.spawn()
    }
}

impl Process for Child {
    fn kill(&mut self) -> std::io::Result<()> {
        self.kill()
    }
}

#[derive(Debug)]
struct ProcessRunner<C = StandardCommand, P = Child>
where
    C: Command,
    P: Process,
{
    process_command: C,
    process_handle: Option<P>,
}

impl ProcessRunner {
    fn new<I, S>(binary_path: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = StandardCommand::new(binary_path);
        command.args(args);

        Self {
            process_command: command,
            process_handle: None,
        }
    }
}

impl<C, P> CommandExecutor for ProcessRunner<C, P>
where
    C: Command<Proc = P>,
    P: Process,
{
    type Error = CommandError;

    fn start(&mut self) -> Result<(), Self::Error> {
        if self.process_handle.is_some() {
            return Err(CommandError::ProcessAlreadyStarted);
        }

        Ok(self.process_handle = Some(self.process_command.spawn()?))
    }

    fn stop(&mut self) -> Result<(), CommandError> {
        if let Some(mut child) = self.process_handle.take() {
            Ok(child.kill()?)
        } else {
            Err(CommandError::ProcessNotStarted)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::command::CommandExecutor;

    use super::ProcessRunner;

    struct MockedCommand;
    struct MockedProc;

    impl super::Process for MockedProc {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl super::Command for MockedCommand {
        type Proc = MockedProc;
        fn spawn(&mut self) -> std::io::Result<MockedProc> {
            Ok(MockedProc {})
        }
    }

    #[test]
    fn start_stop_workflow() {
        let mut cmd: ProcessRunner<MockedCommand, MockedProc> = ProcessRunner {
            process_command: MockedCommand,
            process_handle: None,
        };

        // double start
        assert_eq!(false, cmd.start().is_err());
        assert_eq!(true, cmd.start().is_err());

        // double stop
        assert_eq!(false, cmd.stop().is_err());
        assert_eq!(true, cmd.stop().is_err());

        // start after stop
        assert_eq!(false, cmd.start().is_err())
    }
}
