use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};

use crate::{
    command::stream::Event,
    config::agent_configs::AgentID,
    config::{
        agent_configs::SuperAgentConfig,
        agent_type::OnHost,
        agent_type_registry::{AgentRepository, LocalRepository},
    },
    supervisor::{
        error::ProcessError,
        runner::{Running, Stopped, SupervisorRunner},
        supervisor::Config,
        Handle, Runner,
    },
};

#[derive(Default)]
pub struct SupervisorGroup<S>(HashMap<AgentID, Vec<SupervisorRunner<S>>>);

impl SupervisorGroup<Stopped> {
    pub fn new(
        tx: Sender<Event>,
        cfg: &SuperAgentConfig,
        effective_agent_repository: LocalRepository,
    ) -> Self {
        let builder = SupervisorGroupBuilder {
            tx,
            cfg: cfg.clone(),
            effective_agent_repository,
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
    pub fn wait(self) -> HashMap<AgentID, Vec<WaitResult>> {
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

    pub fn stop(self) -> HashMap<AgentID, Vec<JoinHandle<()>>> {
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

struct SupervisorGroupBuilder {
    tx: Sender<Event>,
    cfg: SuperAgentConfig,
    effective_agent_repository: LocalRepository,
}

impl From<&SupervisorGroupBuilder> for SupervisorGroup<Stopped> {
    fn from(builder: &SupervisorGroupBuilder) -> Self {
        let agent_runners = builder
            .cfg
            .agents
            .iter()
            .map(|(agent_t, agent_cfg)| {
                let agent = builder
                    .effective_agent_repository
                    .get(&agent_t.clone().get());
                if let Some(on_host) = &agent.unwrap().meta.deployment.on_host {
                    return Self::build_on_host_runners(&builder.tx, agent_t, on_host.clone());
                }
                (agent_t.clone(), Vec::new())
            })
            .collect();

        SupervisorGroup(agent_runners)
    }
}

impl SupervisorGroup<Stopped> {
    fn build_on_host_runners(
        tx: &Sender<Event>,
        agent_t: &AgentID,
        on_host: OnHost,
    ) -> (AgentID, Vec<SupervisorRunner>) {
        let mut runners = Vec::new();
        for exec in on_host.executables {
            let runner = SupervisorRunner::from(&Config::new(
                exec.path.clone(),
                exec.args.into_vector().clone(),
                exec.env.into_map().clone(),
                tx.clone(),
                on_host.restart_policy.clone(),
            ));
            runners.push(runner);
        }
        (agent_t.clone(), runners)
    }
}

#[cfg(test)]
pub mod tests {
    use std::{collections::HashMap, sync::mpsc::Sender};

    use crate::config::agent_configs::SuperAgentConfig;
    use crate::config::agent_type_registry::LocalRepository;
    use crate::{
        command::stream::Event,
        config::agent_configs::AgentID,
        supervisor::runner::{
            sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
        },
    };

    use super::{SupervisorGroup, SupervisorGroupBuilder};

    // new_sleep_supervisor_group returns a stopped supervisor group with 2 runners with
    // generic agents one with one exec and the other with 2
    pub fn new_sleep_supervisor_group(tx: Sender<Event>) -> SupervisorGroup<Stopped> {
        let group: HashMap<AgentID, Vec<SupervisorRunner<Stopped>>> = HashMap::from([
            (
                AgentID("sleep_5".to_string()),
                vec![new_sleep_supervisor(tx.clone(), 5)],
            ),
            (
                AgentID("sleep_10".to_string()),
                vec![
                    new_sleep_supervisor(tx.clone(), 10),
                    new_sleep_supervisor(tx.clone(), 10),
                ],
            ),
        ]);
        SupervisorGroup(group)
    }
}
