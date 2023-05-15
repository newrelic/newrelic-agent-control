use crate::{InfraAgentSupervisor, NrDotSupervisor, Supervisor};
use crate::supervisor::generic::factory::GenericSupervisorFactory;
use crate::supervisor::supervisor::{Result, SupervisorError};

pub struct SupervisorFactory {}

impl SupervisorFactory {
    pub fn from_config(stype: String, raw_conf: String) -> Result<Box<dyn Supervisor>> {
        match stype.as_str() {
            "infra_agent" => Ok(Box::new(InfraAgentSupervisor::new())),
            "nrdot" => Ok(Box::new(NrDotSupervisor::new())),
            _ => {
                match GenericSupervisorFactory::from_config(raw_conf) {
                    Err(e) => Err(SupervisorError::Error(e.to_string())),
                    Ok(x) => Ok(Box::new(x))
                }
            }
        }
    }
}

