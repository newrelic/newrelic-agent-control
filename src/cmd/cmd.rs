use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::mpsc::Sender;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

pub struct Cmd {
    command: String,
    process_command: Command,
    process_handle: Option<Child>,
    pid: i32,
}

impl Cmd {
    fn new(command: &str) -> Cmd {
        Cmd {
            command: command.to_string(),
            process_command: Command::new(command),
            process_handle: None,
            pid: 0,
        }
    }
    fn start(&mut self) {
        //self.process_handle = &(self.process_command.spawn());
        let handle = self.process_command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        match handle {
            Ok(c) => self.process_handle = Some(c),
            Err(e) => println!("{}", e),
        }

        let child = self.process_handle.as_mut().unwrap();
        self.pid = child.id() as i32;
    }

    fn stdout(&mut self) -> Option<ChildStdout> {
        self.process_handle.as_mut().unwrap().stdout.take()
    }
    fn stderr(&mut self) -> Option<ChildStderr> {
        self.process_handle.as_mut().unwrap().stderr.take()
    }

    fn stop(&mut self) {
        self.sigkill();
        println!("stop")
    }
    fn sigterm(&mut self) {
        signal::kill(Pid::from_raw(self.pid), Signal::SIGTERM).unwrap();
        println!("sigterm")
    }
    fn sigkill(&mut self) {
        signal::kill(Pid::from_raw(self.pid), Signal::SIGKILL).unwrap();
        println!("sigkill")
    }
    fn is_alive() {}
}

fn std_to_chan<T: Read>(std: Option<T> ,chan: Sender<String>){
    let std_reader = BufReader::new(std.unwrap());
    let std_lines = std_reader.lines();

    for line in std_lines {
        chan.send(line.unwrap());
    }
}