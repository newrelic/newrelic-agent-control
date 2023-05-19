use std::{
    ffi::OsStr,
    process::{Child, Command as StandardCommand, ExitStatus},
};

use super::{Command, CommandError, CommandExecutor, CommandHandler, CommandRunner, Process};

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

    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.wait()
    }
}

pub struct Unstarted<C, P>
where
    C: Command<Proc = P>,
{
    cmd: C,
}

pub struct Started<P>
where
    P: Process,
{
    process_handle: P,
}

#[derive(Debug)]
pub struct ProcessRunner<State = Unstarted<StandardCommand, Child>> {
    state: State,
}

impl ProcessRunner {
    pub fn new<I, S>(binary_path: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = StandardCommand::new(binary_path);
        command.args(args);

        Self {
            state: Unstarted { cmd: command },
        }
    }
}

impl<C, P> CommandExecutor for ProcessRunner<Unstarted<C, P>>
where
    C: Command<Proc = P>,
    P: Process,
{
    type Error = CommandError;
    type Handler = ProcessRunner<Started<P>>;

    fn start(mut self) -> Result<Self::Handler, Self::Error> {
        let child = self.state.cmd.spawn()?;
        Ok(ProcessRunner {
            state: Started {
                process_handle: child,
            },
        })
    }
}

impl<P> CommandHandler for ProcessRunner<Started<P>>
where
    P: Process,
{
    type Error = CommandError;
    fn stop(mut self) -> Result<(), CommandError> {
        Ok(self.state.process_handle.kill()?)
    }
}

impl<C, P> CommandRunner for ProcessRunner<Unstarted<C, P>>
where
    C: Command<Proc = P>,
    P: Process,
{
    type Error = CommandError;
    fn run(mut self) -> Result<ExitStatus, Self::Error> {
        Ok(self.state.cmd.spawn()?.wait()?)
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use crate::command::{processrunner::Unstarted, CommandExecutor, CommandHandler};

    use super::ProcessRunner;

    struct MockedCommand;
    struct MockedProc;

    impl super::Process for MockedProc {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
            Ok(ExitStatus::from_raw(0))
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
        let cmd: ProcessRunner<Unstarted<MockedCommand, MockedProc>> = ProcessRunner {
            state: Unstarted { cmd: MockedCommand },
        };

        assert_eq!(cmd.start().unwrap().stop().is_err(), false);
    }
}
