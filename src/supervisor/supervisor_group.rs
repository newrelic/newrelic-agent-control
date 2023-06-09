use std::{collections::HashMap, sync::mpsc::Sender};

use crate::{
    command::stream::OutputEvent,
    config::{agent_configs::MetaAgentConfig, agent_type::AgentType},
};

use super::{
    context::SupervisorContext,
    error::ProcessError,
    newrelic_infra_supervisor::NRIConfig,
    nrdot_supervisor::NRDOTConfig,
    runner::{Running, Stopped, SupervisorRunner},
    Handle, Runner,
};

pub struct SupervisorGroup<S>(HashMap<AgentType, SupervisorRunner<S>>);

impl SupervisorGroup<Stopped> {
    pub fn new(ctx: SupervisorContext, tx: Sender<OutputEvent>, cfg: &MetaAgentConfig) -> Self {
        let builder = SupervisorGroupBuilder {
            ctx,
            tx,
            cfg: cfg.clone(),
        };
        SupervisorGroup::from(&builder)
    }

    pub fn run(self) -> SupervisorGroup<Running> {
        let running = self
            .0
            .into_iter()
            .map(|(t, runner)| (t, runner.run()))
            .collect();
        SupervisorGroup(running)
    }
}

type WaitResult = Result<(), ProcessError>;
impl SupervisorGroup<Running> {
    pub fn wait(self) -> HashMap<AgentType, WaitResult> {
        self.0
            .into_iter()
            .map(|(t, runner)| (t, runner.wait()))
            .collect()
    }
}

struct SupervisorGroupBuilder {
    ctx: SupervisorContext,
    tx: Sender<OutputEvent>,
    cfg: MetaAgentConfig,
}

impl From<&SupervisorGroupBuilder> for SupervisorGroup<Stopped> {
    fn from(value: &SupervisorGroupBuilder) -> Self {
        let runners = value
            .cfg
            .agents
            .keys() // When we make the agents configurable, we'll need to iterate over both the keys and values
            .map(|agent_type| {
                let agent_type = agent_type.clone();
                let ctx = value.ctx.clone();
                let tx = value.tx.clone();
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
        SupervisorGroup(runners)
    }
}
