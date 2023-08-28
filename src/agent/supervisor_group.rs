use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use crate::{
    command::stream::Event,
    config::agent_configs::SuperAgentConfig,
    supervisor::{
        error::ProcessError,
        supervisor::Config,
        runner::{Running, Stopped, SupervisorRunner},
        Handle, Runner,
    },
};
use crate::config::agent_configs::AgentConfig;
use crate::config::agent_type::Executable;
use crate::config::agent_type_registry::AgentRepository;

pub struct SupervisorGroup<S>(HashMap<String, Vec<SupervisorRunner<S>>>);

impl SupervisorGroup<Stopped> {
    pub fn new<R: AgentRepository>(tx: Sender<Event>, cfg: &SuperAgentConfig, agent_types: R) -> Self {
        let builder = SupervisorGroupBuilder {
            tx,
            cfg: cfg.clone(),
            agent_repository: agent_types,
        };
        SupervisorGroup::from(&builder)
    }

    pub fn run(self) -> SupervisorGroup<Running> {
        let running = self
            .0
            .into_iter()
            .map(|(t, runners)| {
                let mut running_runners = Vec::new();
                for runner in runners {
                    running_runners.push(runner.run());
                }
                (t, running_runners)
            })
            .collect();
        SupervisorGroup(running)
    }
}

type WaitResult = Result<(), ProcessError>;
impl SupervisorGroup<Running> {
    pub fn wait(self) -> HashMap<String, Vec<WaitResult>> {
        self.0
            .into_iter()
            .map(|(t, runners)| {
                let mut waiting_runners = Vec::new();
                for runner in runners {
                    waiting_runners.push(runner.wait());
                }
                (t, waiting_runners)
            })
            .collect()
    }

    pub fn stop(self) -> HashMap<String, Vec<JoinHandle<()>>> {
        self.0
            .into_iter()
            .map(|(t, runners)| {
                let mut stopped_runners = Vec::new();
                for runner in runners {
                    stopped_runners.push(runner.stop());
                }
                (t, stopped_runners)
            })
            .collect()
    }
}

struct SupervisorGroupBuilder<R: AgentRepository> {
    tx: Sender<Event>,
    cfg: SuperAgentConfig,
    agent_repository: R,
}

impl<R: AgentRepository> From<&SupervisorGroupBuilder<R>> for SupervisorGroup<Stopped> {
    fn from(builder: &SupervisorGroupBuilder<R>) -> Self {
        let agent_runners = builder
            .cfg
            .agents
            .iter()
            .map(|(agent_t, agent_cfg)| {
                let agent = builder.agent_repository.get(agent_t);
                if let Some(on_host) = &agent.unwrap().meta.deployment.on_host {
                    if let Some(host_runners) = Self::build_on_host_runners(&builder.tx, agent_t, agent_cfg.clone().unwrap_or_default(), on_host.executables.clone()) {
                        return host_runners;
                    }
                }
                (agent_t.clone(), Vec::new())
            })
            .collect();

        SupervisorGroup(agent_runners)
    }

}

impl SupervisorGroup<Stopped> {
    fn build_on_host_runners(tx: &Sender<Event>, agent_t: &String, agent_cfg: AgentConfig, execs: Vec<Executable>) -> Option<(String, Vec<SupervisorRunner>)> {
        let mut runners = Vec::new();
        for exec in execs {
            let runner = SupervisorRunner::from(
                &Config::new(
                    exec.path.clone(),
                    exec.args.clone(),
                    exec.env.clone(),
                    tx.clone(),
                    agent_cfg.clone(),
                )
            );
            runners.push(runner);
        }
        Some((agent_t.clone(), runners))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::HashMap, sync::mpsc::Sender};

    use crate::{
        command::stream::Event,
        config::agent_type_registry::AgentRepository,
        supervisor::runner::{
            sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
        },
    };

    use super::SupervisorGroup;

    // new_sleep_supervisor_group returns a stopped supervisor group with 2 runners with
    // generic agents one with one exec and the other with 2
    pub(crate) fn new_sleep_supervisor_group(tx: Sender<Event>) -> SupervisorGroup<Stopped> {
        let group: HashMap<String, Vec<SupervisorRunner<Stopped>>> = HashMap::from([
            (
                "sleep_5".to_string(),
                vec![new_sleep_supervisor(tx.clone(), 5)],
            ),
            (
                "sleep_10".to_string(),
                vec![
                    new_sleep_supervisor(tx.clone(), 10),
                    new_sleep_supervisor(tx.clone(), 10)
                ],
            ),
        ]);
        SupervisorGroup(group)
    }
}
