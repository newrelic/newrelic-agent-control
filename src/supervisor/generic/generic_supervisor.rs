use crate::{Cmd, Supervisor};
use crate::cmd::cmd::cmd_channels;
use crate::supervisor::supervisor::Result;
use crate::supervisor::supervisor::SupervisorError::Error;

pub struct GenericSupervisor {
    cmd: Cmd,
}

impl GenericSupervisor {
    pub fn new(cmd: Cmd) -> Self {
        Self { cmd }
    }

    // pub fn cmd(&mut self) -> &mut Cmd {
    //     &mut self.cmd
    // }
}

impl Supervisor for GenericSupervisor {
    fn start(&mut self) -> Result<()> {
        self.cmd.start()?;

        cmd_channels(&mut self.cmd);

        match self.cmd.wait() {
            Err(e) => Err(Error(e.to_string())),
            Ok(e) => {
                println!("generic supervisor agent exited. code: {}", e.to_string());
                Ok(())
            }
        }
    }
}