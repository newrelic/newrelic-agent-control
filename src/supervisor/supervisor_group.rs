use std::{collections::HashMap, sync::mpsc::Receiver};

use crate::{
    command::stream::OutputEvent,
    config::{agent_configs::MetaAgentConfig, agent_type::AgentType},
};

use super::{
    context::SupervisorContext, newrelic_infra_supervisor::NRIConfig,
    nrdot_supervisor::NRDOTConfig, runner::SupervisorRunner,
};

pub struct SupervisorGroup {
    _receiver: Receiver<OutputEvent>,
    _context: SupervisorContext,
    _runners: HashMap<AgentType, SupervisorRunner>,
}

impl SupervisorGroup {}

impl From<&MetaAgentConfig> for SupervisorGroup {
    fn from(value: &MetaAgentConfig) -> Self {
        let (tx, _receiver) = std::sync::mpsc::channel();
        let _context = SupervisorContext::new();

        let _runners = value
            .agents
            .keys()
            .map(|agent_type| {
                let agent_type = agent_type.clone();
                let ctx = _context.clone();
                let tx = tx.clone();
                let runner = match agent_type {
                    AgentType::InfraAgent(_) => SupervisorRunner::from(&NRIConfig::new(ctx, tx)),
                    AgentType::Nrdot(_) => SupervisorRunner::from(&NRDOTConfig::new(ctx, tx)),
                    AgentType::Custom(_, _) => {
                        unimplemented!("Custom agent type not implemented yet")
                    }
                };
                (agent_type, runner)
            })
            .collect();
        Self {
            _receiver,
            _context,
            _runners,
        }
    }
}
