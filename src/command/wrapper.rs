use std::{
    marker::PhantomData,
    process::{Child, Command},
};

use super::{CommandError, CommandExecutor, CommandHandle, CommandRunner};

impl CommandExecutor for Command {
    type Error = CommandError;
    type Process = Child;
    fn start(mut self) -> Result<Self::Process, Self::Error> {
        Ok(self.spawn()?)
    }
}

impl CommandHandle for Child {
    type Error = CommandError;
    fn stop(mut self) -> Result<(), Self::Error> {
        Ok(self.kill()?)
    }
}

pub enum ProcessRunner {
    Command(Command),
    Child(Child),
}

impl ProcessRunner {
    pub fn new(cmd: Command) -> Self {
        ProcessRunner::Command(cmd)
    }
}

impl CommandExecutor for ProcessRunner {
    type Error = CommandError;
    type Process = Child;
    fn start(self) -> Result<Self::Process, Self::Error> {
        match self {
            ProcessRunner::Command(mut cmd) => Ok(cmd.spawn()?),
            _ => unreachable!(),
        }
    }
}

impl CommandHandle for ProcessRunner {
    type Error = CommandError;
    fn stop(self) -> Result<(), Self::Error> {
        match self {
            ProcessRunner::Child(mut cmd) => Ok(cmd.kill()?),
            _ => unreachable!(),
        }
    }
}

impl CommandRunner for ProcessRunner {
    type Error = CommandError;
    fn run(self) -> Result<std::process::ExitStatus, Self::Error> {
        match self {
            ProcessRunner::Command(mut cmd) => Ok(cmd.spawn()?.wait()?),
            _ => unreachable!(),
        }
    }
}

// struct Unstarted;
// struct Started;

// struct ProcessRunner2<State = Unstarted> {
//     cmd: Command,

//     state: PhantomData<State>,
// }

// impl CommandExecutor for ProcessRunner2 {
//     type Error = CommandError;
//     type Process = ProcessRunner2<Started>;
//     fn start(self) -> Result<Self::Process, Self::Error> {
//         Ok(self.cmd.spawn()?)
//     }
// }

// impl CommandHandle for ProcessRunner2<Started> {}

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
