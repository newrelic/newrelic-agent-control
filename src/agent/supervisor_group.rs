use std::{collections::HashMap, sync::mpsc::Sender};

use crate::{
    command::stream::Event,
    config::{agent_configs::MetaAgentConfig, agent_type::AgentType},
    supervisor::{
        context::SupervisorContext,
        error::ProcessError,
        newrelic_infra_supervisor::NRIConfig,
        nrdot_supervisor::NRDOTConfig,
        runner::{Running, Stopped, SupervisorRunner},
        Handle, Runner,
    },
};

pub struct SupervisorGroup<S>(HashMap<AgentType, SupervisorRunner<S>>);

impl SupervisorGroup<Stopped> {
    pub fn new(ctx: SupervisorContext, tx: Sender<Event>, cfg: &MetaAgentConfig) -> Self {
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
    tx: Sender<Event>,
    cfg: MetaAgentConfig,
}

impl From<&SupervisorGroupBuilder> for SupervisorGroup<Stopped> {
    fn from(value: &SupervisorGroupBuilder) -> Self {
        let runners = value
            .cfg
            .agents
            .iter()
            .map(|(agent_t, agent_cfg)| {
                let ctx = value.ctx.clone();
                let tx = value.tx.clone();
                let cfg = agent_cfg.clone();
                let runner = match &agent_t {
                    AgentType::InfraAgent(_) => {
                        SupervisorRunner::from(&NRIConfig::new(ctx, tx, cfg))
                    }
                    AgentType::Nrdot(_) => SupervisorRunner::from(&NRDOTConfig::new(ctx, tx, cfg)),
                    AgentType::Custom(_, _) => {
                        unimplemented!("Custom agent type not implemented yet")
                    }
                };
                (agent_t.clone(), runner)
            })
            .collect();
        SupervisorGroup(runners)
    }
}
