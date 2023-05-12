use std::{thread, time};
use std::sync::mpsc;
use std::thread::sleep;

use crate::Cmd;
use crate::cmd::cmd::std_to_chan;
use crate::supervisor::supervisor::Supervisor;

pub struct InfraAgentSupervisor {
    cmd: Cmd,
}

impl Supervisor for InfraAgentSupervisor {
    fn start(&mut self) -> crate::supervisor::supervisor::Result<()> {
        println!("starting supervisor");


        self.cmd.start()?;

        let stdout = self.cmd.stdout();
        let stderr = self.cmd.stderr();

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
        self.cmd.stop();

        println!("stopping processes");

        Ok(())
    }
}

impl InfraAgentSupervisor {
    pub fn new() -> Self {
        let cmd = Cmd::new("/usr/bin/newrelic-infra-service");
        Self { cmd }
    }
}



