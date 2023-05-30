use std::sync::mpsc::Sender;
use std::sync::Mutex;

use crate::ctx::Ctx;
use crate::InfraAgentSupervisorRunner;
use crate::stream::OutputEvent;
use crate::supervisor::supervisor::{Result, SupervisorError, SupervisorExecutor, SupervisorHandle, SupervisorRunner};

pub struct SupervisorFactory {}

impl SupervisorFactory where {
    pub fn from_config(stype: String, std_sender: Mutex<Sender<OutputEvent>>) -> Result<Box<dyn SupervisorExecutor<Supervisor=dyn SupervisorHandle> + Sync>> {
        match stype.as_str() {
            "infra_agent" => Ok(Box::new(InfraAgentSupervisorRunner::new(std_sender))),
            // "nrdot" => Ok(Box::new(NrDotSupervisor::new())),
            unsupported => {
                Err(SupervisorError::Error(format!("supervisor type {} not supported", unsupported)))
            }
        }
    }
}

