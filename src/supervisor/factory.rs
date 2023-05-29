use std::sync::mpsc::Sender;
use std::sync::Mutex;

use crate::{InfraAgentSupervisor, Supervisor};
use crate::ctx::Ctx;
use crate::stream::OutputEvent;
use crate::supervisor::supervisor::{Result, SupervisorError};

pub struct SupervisorFactory {}

impl SupervisorFactory where {
    pub fn from_config<C>(ctx: C, stype: String, _: String, std_sender: Mutex<Sender<OutputEvent>>) -> Result<Box<dyn Supervisor + Sync>> where C: Ctx + Send + Sync + Clone + 'static {
        match stype.as_str() {
            "infra_agent" => Ok(Box::new(InfraAgentSupervisor::new(ctx, std_sender))),
            // "nrdot" => Ok(Box::new(NrDotSupervisor::new())),
            unsupported => {
                Err(SupervisorError::Error(format!("supervisor type {} not supported", unsupported)))
            }
        }
    }
}

