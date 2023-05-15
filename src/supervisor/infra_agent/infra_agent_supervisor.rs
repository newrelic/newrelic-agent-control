use crate::Cmd;
use crate::cmd::cmd::cmd_channels;
use crate::supervisor::supervisor::{Supervisor};
use crate::supervisor::supervisor::SupervisorError::Error;

pub struct InfraAgentSupervisor {
    cmd: Cmd,
}

impl Supervisor for InfraAgentSupervisor {
    fn start(&mut self) -> crate::supervisor::supervisor::Result<()> {
        println!("starting infra agent supervisor");


        self.cmd.start()?;

        cmd_channels(&mut self.cmd);

        match self.cmd.wait() {
            Err(e) => Err(Error(e.to_string())),
            Ok(e) => {
                println!("infra-agent supervisor agent exited. code: {}", e.to_string());
                Ok(())
            }
        }
    }
}

impl InfraAgentSupervisor {
    pub fn new() -> Self {
        let cmd = Cmd::new("/usr/bin/newrelic-infra-service", Vec::new());
        Self { cmd }
    }
}



