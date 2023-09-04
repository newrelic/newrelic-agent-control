use std::{collections::HashMap, sync::mpsc::Sender, thread::JoinHandle};
use tracing::debug;

use crate::{
    command::stream::Event,
    config::agent_configs::AgentID,
    config::{
        agent_configs::SuperAgentConfig, agent_type::OnHost, agent_type_registry::AgentRepository,
    },
    supervisor::{
        error::ProcessError,
        runner::{Running, Stopped, SupervisorRunner},
        supervisor_config::Config,
        Handle, Runner,
    },
};

#[derive(Default)]
pub struct SupervisorGroup<S>(HashMap<AgentID, Vec<SupervisorRunner<S>>>);

impl SupervisorGroup<Stopped> {
    pub fn new<Repo: AgentRepository>(
        tx: Sender<Event>,
        cfg: &SuperAgentConfig,
        effective_agent_repository: Repo,
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

struct SupervisorGroupBuilder<Repo> {
    tx: Sender<Event>,
    cfg: SuperAgentConfig,
    effective_agent_repository: Repo,
}

impl<Repo> From<&SupervisorGroupBuilder<Repo>> for SupervisorGroup<Stopped>
where
    Repo: AgentRepository,
{
    fn from(builder: &SupervisorGroupBuilder<Repo>) -> Self {
        let agent_runners = builder
            .cfg
            .agents
            .keys()
            .map(|agent_t| {
                let agent = builder
                    .effective_agent_repository
                    .get(&agent_t.clone().get());
                match agent {
                    Ok(agent) => {
                        if let Some(on_host) = &agent.runtime_config.deployment.on_host {
                            return Self::build_on_host_runners(
                                &builder.tx,
                                agent_t,
                                on_host.clone(),
                            );
                        }
                        return (agent_t.clone(), Vec::new());
                    }
                    Err(error) => {
                        debug!("repository error: {}", error);
                        (agent_t.clone(), Vec::new())
                    }
                }
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
                exec.path,
                exec.args.into_vector(),
                exec.env.into_map(),
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

    use super::{SupervisorGroup, SupervisorGroupBuilder};
    use crate::config::agent_type::RuntimeConfig;
    use crate::{
        command::stream::Event,
        config::agent_configs::{AgentID, AgentSupervisorConfig, SuperAgentConfig},
        config::agent_type::{Agent, Deployment, Executable, OnHost},
        config::agent_type_registry::{AgentRepository, LocalRepository},
        supervisor::runner::{
            sleep_supervisor_tests::new_sleep_supervisor, Stopped, SupervisorRunner,
        },
    };

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

    #[test]
    fn new_supervisor_group_from() {
        let (tx, _) = std::sync::mpsc::channel();
        let agent_config = SuperAgentConfig {
            agents: HashMap::from([
                (
                    AgentID("no_repository_key".to_string()),
                    AgentSupervisorConfig {
                        agent_type: "".to_string(),
                        values_file: "".to_string(),
                    },
                ),
                (
                    AgentID("no_data".to_string()),
                    AgentSupervisorConfig {
                        agent_type: "".to_string(),
                        values_file: "".to_string(),
                    },
                ),
                (
                    AgentID("full_data".to_string()),
                    AgentSupervisorConfig {
                        agent_type: "".to_string(),
                        values_file: "".to_string(),
                    },
                ),
            ]),
        };

        let mut builder = SupervisorGroupBuilder {
            tx,
            cfg: agent_config.clone(),
            effective_agent_repository: LocalRepository::default(),
        };
        _ = builder.effective_agent_repository.store_with_key(
            "no_data".to_string(),
            Agent {
                metadata: Default::default(),
                variables: Default::default(),
                runtime_config: Default::default(),
            },
        );
        _ = builder.effective_agent_repository.store_with_key(
            "full_data".to_string(),
            Agent {
                metadata: Default::default(),
                variables: Default::default(),
                runtime_config: RuntimeConfig {
                    deployment: Deployment {
                        on_host: Some(OnHost {
                            executables: vec![Executable {
                                path: "a-path".to_string(),
                                args: Default::default(),
                                env: Default::default(),
                            }],
                            restart_policy: Default::default(),
                        }),
                    },
                },
            },
        );

        let supervisor_group = SupervisorGroup::from(&builder);
        assert_eq!(supervisor_group.0.iter().count(), 3)
    }
}
