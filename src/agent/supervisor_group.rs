use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use crate::{
    command::stream::Event,
    config::{agent_configs::SuperAgentConfig, agent_definition::AgentDefinition},
    supervisor::{
        error::ProcessError,
        newrelic_infra_supervisor::NRIConfig,
        nrdot_supervisor::NRDOTConfig,
        runner::{Running, Stopped, SupervisorRunner},
        Handle, Runner,
    },
};

pub struct SupervisorGroup<S>(HashMap<AgentDefinition, SupervisorRunner<S>>);

impl SupervisorGroup<Stopped> {
    pub fn new(tx: Sender<Event>, cfg: &SuperAgentConfig) -> Self {
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
    pub fn wait(self) -> HashMap<AgentDefinition, WaitResult> {
        self.0
            .into_iter()
            .map(|(t, runner)| (t, runner.wait()))
            .collect()
    }

    pub fn stop(self) -> HashMap<AgentDefinition, JoinHandle<()>> {
        self.0
            .into_iter()
            .map(|(t, runner)| (t, runner.stop()))
            .collect()
    }
}

struct SupervisorGroupBuilder {
    tx: Sender<Event>,
    cfg: SuperAgentConfig,
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
                    AgentDefinition::InfraAgent(_) => {
                        SupervisorRunner::from(&NRIConfig::new(tx, cfg))
                    }
                    AgentDefinition::Nrdot(_) => SupervisorRunner::from(&NRDOTConfig::new(tx, cfg)),
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
        config::agent_definition::AgentDefinition,
        supervisor::runner::{
            sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
        },
    };

    use super::SupervisorGroup;

    // new_sleep_supervisor_group returns a stopped supervisor group with to runners which mock the
    // InfraAgent by sleeping 5 and 10 seconds respectively
    pub(crate) fn new_sleep_supervisor_group(tx: Sender<Event>) -> SupervisorGroup<Stopped> {
        let group: HashMap<AgentDefinition, SupervisorRunner<Stopped>> = HashMap::from([
            (
                AgentDefinition::InfraAgent(Some("sleep_5".to_string())),
                new_sleep_supervisor(tx.clone(), 5),
            ),
            (
                AgentDefinition::InfraAgent(Some("sleep_10".to_string())),
                new_sleep_supervisor(tx, 10),
            ),
        ]);
        SupervisorGroup(group)
    }
}
