use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use crate::{
    command::stream::Event,
    config::{agent_configs::MetaAgentConfig, agent_type::AgentType},
    supervisor::{
        error::ProcessError,
        newrelic_infra_supervisor::NRIConfig,
        nrdot_supervisor::NRDOTConfig,
        runner::{Running, Stopped, SupervisorRunner},
        Handle, Runner,
    },
};

pub struct SupervisorGroup<S>(HashMap<AgentType, SupervisorRunner<S>>);

impl SupervisorGroup<Stopped> {
    pub fn new(tx: Sender<Event>, cfg: &MetaAgentConfig) -> Self {
        let builder = SupervisorGroupBuilder {
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

    pub fn stop(self) -> HashMap<AgentType, JoinHandle<()>> {
        self.0
            .into_iter()
            .map(|(t, runner)| (t, runner.stop()))
            .collect()
    }
}

struct SupervisorGroupBuilder {
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
                let tx = value.tx.clone();
                let cfg = agent_cfg.clone().unwrap_or_default();
                let runner = match &agent_t {
                    AgentType::InfraAgent(_) => {
                        SupervisorRunner::from(&NRIConfig::new(tx, String::from(agent_t), cfg))
                    }
                    AgentType::Nrdot(_) => {
                        SupervisorRunner::from(&NRDOTConfig::new(tx, String::from(agent_t), cfg))
                    }
                };
                (agent_t.clone(), runner)
            })
            .collect();
        SupervisorGroup(runners)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::HashMap, sync::mpsc::Sender};

    use crate::{
        command::stream::Event,
        config::agent_type::AgentType,
        supervisor::runner::{
            sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
        },
    };

    use super::SupervisorGroup;

    // new_sleep_supervisor_group returns a stopped supervisor group with to runners which mock the
    // InfraAgent by sleeping 5 and 10 seconds respectively
    pub(crate) fn new_sleep_supervisor_group(tx: Sender<Event>) -> SupervisorGroup<Stopped> {
        let group: HashMap<AgentType, SupervisorRunner<Stopped>> = HashMap::from([
            (
                AgentType::InfraAgent(Some("sleep_5".to_string())),
                new_sleep_supervisor(tx.clone(), 5),
            ),
            (
                AgentType::InfraAgent(Some("sleep_10".to_string())),
                new_sleep_supervisor(tx, 10),
            ),
        ]);
        SupervisorGroup(group)
    }
}
