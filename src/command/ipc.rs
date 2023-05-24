use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use libc::{SIGKILL, SIGTERM, SIGUSR1, SIGUSR2};

use super::{CommandError, CommandNotifier};

pub struct ProcessNotifier{
    pid: i32
}

impl ProcessNotifier {
    pub fn new(pid:i32) -> Self
    {
        Self { pid }
    }
}

impl CommandNotifier for ProcessNotifier{
    type Error = CommandError;

    #[cfg(target_family = "unix")]
    fn notify(&self, msg:Message) -> Result<(), Self::Error> {
        let result_signal = signal::kill(Pid::from_raw(self.pid), msg);
        let result = match result_signal {
            Ok(signal) => Ok(signal),
            Err(error) => Err(CommandError::from(error)),
        };
        result
    }

    #[cfg(not(target_family = "unix"))]
    fn notify(msg:Message) -> Result<(), Self::Error> {
        unimplemented!("windows processes can't be notified")
    }
}

#[repr(i32)]
#[cfg(target_family = "unix")]
pub enum Message {
    NotificationA = SIGUSR1,
    NotificationB = SIGUSR2,
    Kill = SIGKILL,
    Term = SIGTERM,
}

#[cfg(target_family = "unix")]
impl From<Message> for Option<Signal> {
    fn from(value: Message) -> Option<Signal> {
        match value {
            Message::NotificationA => Some(Signal::SIGUSR1),
            Message::NotificationB => Some(Signal::SIGUSR2),
            Message::Kill => Some(Signal::SIGKILL),
            Message::Term => Some(Signal::SIGTERM),
        }
    }
}

#[cfg(not(target_family = "unix"))]
pub enum Message {}

#[cfg(target_family = "unix")]
#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::{thread, time};
    use std::io::{BufRead, BufReader};
    use crate::command::ipc::Message::NotificationA;
    use super::*;

    #[test]
    fn notify_process() {
        let mut sleep_cmd = Command::new("sh")
            .arg("-c")
            .arg("trap \"echo 'sigusr1 signal captured'\" SIGUSR1;while true; do sleep 1; done")
            .stdout(Stdio::piped())
            .spawn();

        let pid = sleep_cmd.as_mut().unwrap().id();
        let one_second = time::Duration::from_secs(1);
        thread::sleep(one_second);

        let notifier = ProcessNotifier::new(pid as i32);
        _ = notifier.notify(NotificationA);

        let std_reader = BufReader::new(sleep_cmd.as_mut().unwrap().stdout.as_mut().unwrap());
        let std_lines = std_reader.lines();

        let mut output = String::new();
        for line in std_lines {
            output = line.unwrap();
            break;
        }

        assert_eq!(output, "sigusr1 signal captured")
    }
}
