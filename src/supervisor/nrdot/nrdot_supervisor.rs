use crate::Cmd;
use crate::cmd::cmd::cmd_channels;
use crate::supervisor::supervisor::{Supervisor};
use crate::supervisor::supervisor::SupervisorError::Error;

pub struct NrDotSupervisor {
    cmd: Cmd,
}

impl Supervisor for NrDotSupervisor {
    fn start(&mut self) -> crate::supervisor::supervisor::Result<()> {
        println!("starting nrdot supervisor");


        self.cmd.start()?;

        cmd_channels(&mut self.cmd);

        match self.cmd.wait() {
            Err(e) => Err(Error(e.to_string())),
            Ok(e) => {
                println!("nrdot supervisor agent exited. code: {}", e.to_string());
                Ok(())
            }
        }
    }
}

impl NrDotSupervisor {
    pub fn new() -> Self {
        let cmd = Cmd::new(
            "/usr/bin/nr-otel-collector",
            vec![
                "--config".to_string(),
                "/etc/nr-otel-collector/config.yaml".to_string(),
            ],
        );
        Self { cmd }
    }
}



