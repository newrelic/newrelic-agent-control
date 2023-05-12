use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::sleep;
use std::{thread, time};
use std::sync::mpsc;
use std::sync::mpsc::Sender;

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

fn main() {
    println!("starting supervisor");

    let mut orig_command = Cmd::new("./loop.sh");

    orig_command.start();
    let stdout = orig_command.stdout();
    let stderr = orig_command.stderr();

    // let clonned_command_2 = command.clone();
    sleep(time::Duration::from_millis(1000));

    let (stderr_tx, stderr_rx) = mpsc::channel();
    let (stdout_tx, stdout_rx) = mpsc::channel();

    thread::spawn(move || {
        std_to_chan(stdout, stdout_tx);
    });
    thread::spawn(move || {
        std_to_chan(stderr, stderr_tx);
    });

    thread::spawn(move || {
        for msg in stderr_rx {
            println!("stderr channel: {}", msg);
        }
    });
    thread::spawn(move || {
        for msg in stdout_rx {
            println!("stdout channel: {}", msg);
        }
    });

    sleep(time::Duration::from_millis(3000));
    orig_command.stop();

    println!("stopping processes");
}