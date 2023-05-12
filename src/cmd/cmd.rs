use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::result;
use std::sync::mpsc::Sender;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use thiserror::Error;

use crate::cmd::cmd::CmdError::Error;

#[derive(Error, Debug)]
pub enum CmdError {
    #[error("Error running process: {0}")]
    Error(String),
}

/// The result type used by this library, defaulting to [`Error`][crate::Error]
/// as the error type.
pub type Result<T> = result::Result<T, CmdError>;

pub struct Cmd {
    command: String,
    process_command: Command,
    process_handle: Option<Child>,
    pid: i32,
}

impl Cmd {
    pub fn new(command: &str) -> Cmd {
        Cmd {
            command: command.to_string(),
            process_command: Command::new(command),
            process_handle: None,
            pid: 0,
        }
    }
    pub fn start(&mut self) -> Result<()> {
        //self.process_handle = &(self.process_command.spawn());
        let handle = self.process_command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        match handle {
            Err(e) => Err(Error(e.to_string())),
            Ok(c) => {
                self.process_handle = Some(c);
                let child = self.process_handle.as_mut().unwrap();
                self.pid = child.id() as i32;
                Ok(())
            }
        }
    }

    pub fn stdout(&mut self) -> Option<ChildStdout> {
        self.process_handle.as_mut().unwrap().stdout.take()
    }
    pub fn stderr(&mut self) -> Option<ChildStderr> {
        self.process_handle.as_mut().unwrap().stderr.take()
    }

    pub fn stop(&mut self) {
        self.sigkill();
        println!("stop")
    }

    pub fn sigterm(&mut self) {
        signal::kill(Pid::from_raw(self.pid), Signal::SIGTERM).unwrap();
        println!("sigterm")
    }

    pub fn sigkill(&mut self) {
        signal::kill(Pid::from_raw(self.pid), Signal::SIGKILL).unwrap();
        println!("sigkill")
    }

    pub fn is_alive() {}
}

pub fn std_to_chan<T: Read>(std: Option<T>, chan: Sender<String>) {
    let std_reader = BufReader::new(std.unwrap());
    let std_lines = std_reader.lines();

    for line in std_lines {
        chan.send(line.unwrap());
    }
}