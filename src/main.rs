use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::sleep;
use std::{thread, time};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

struct Cmd {
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
        let handle = self.process_command.stdout(Stdio::piped()).spawn();
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

fn main() {
    println!("starting supervisor");

    let mut orig_command = Cmd::new("./loop.sh");

    orig_command.start();
    let stdout = orig_command.stdout();

    // let clonned_command_2 = command.clone();
    sleep(time::Duration::from_millis(1000));

    thread::spawn(move || {
        let stdout_reader = BufReader::new(stdout.unwrap());
        let stdout_lines = stdout_reader.lines();

        for line in stdout_lines {
            println!("Read: {:?}", line);
        }
    });

    sleep(time::Duration::from_millis(3000));
    orig_command.stop();

    println!("stopping processes");
}