```rust

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::sleep;
use std::{thread, time};
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use meta_agent::Cmd;


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
```