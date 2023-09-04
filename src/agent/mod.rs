use std::{
    fs,
    path::Path,
    sync::mpsc::{self, Sender},
};

use tracing::{error, info};

use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{stream::Event, EventLogger, StdEventReceiver},
    config::{
        agent_configs::{AgentID, SuperAgentConfig},
        agent_type_registry::{AgentRepository, LocalRepository},
        resolver::Resolver,
        supervisor_config::SupervisorConfig,
    },
    context::Context,
    supervisor::runner::Stopped,
};

use self::error::AgentError;

pub mod error;
pub mod supervisor_group;

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver<Repo>
where
    Repo: AgentRepository,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
    ) -> SupervisorGroup<Stopped>;
}

impl<Repo> SupervisorGroupResolver<Repo> for SuperAgentConfig
where
    Repo: AgentRepository,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
    ) -> SupervisorGroup<Stopped> {
        SupervisorGroup::new(tx, self, effective_agent_repository)
    }
}

pub struct Agent<Repo, EffectiveRepo = LocalRepository, R = SuperAgentConfig>
where
    R: SupervisorGroupResolver<EffectiveRepo>,
    Repo: AgentRepository,
    EffectiveRepo: AgentRepository,
{
    resolver: R,
    agent_type_repository: Repo,
    effective_agent_repository: EffectiveRepo,
}

impl<Repo> Agent<Repo>
where
    Repo: AgentRepository + Clone,
{
    pub fn new(cfg_path: &Path, agent_type_repository: Repo) -> Result<Self, AgentError> {
        let cfg = Resolver::retrieve_config(cfg_path)?;

        let effective_agent_repository = load_agent_cfgs(&agent_type_repository, &cfg)?;

        Ok(Self {
            resolver: cfg,
            agent_type_repository,
            effective_agent_repository,
        })
    }

    #[cfg(test)]
    fn new_custom_resolver<R>(resolver: R, local_repo: Repo) -> Agent<Repo, R>
    where
        R: SupervisorGroupResolver<LocalRepository>,
    {
        Agent {
            resolver,
            agent_type_repository: local_repo,
            effective_agent_repository: LocalRepository::default(),
        }
    }
}

impl<Repo, EffectiveRepo, R> Agent<Repo, EffectiveRepo, R>
where
    R: SupervisorGroupResolver<EffectiveRepo>,
    Repo: AgentRepository,
    EffectiveRepo: AgentRepository,
{
    pub fn run(self, ctx: Context<Option<AgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        let supervisor_group = self
            .resolver
            .retrieve_group(tx, self.effective_agent_repository);
        /*
            TODO: We should first compare the current config with the one in the super agent config.
            In a future situation, it might have changed due to updates from OpAMP, etc.
            Then, this would require selecting the agents whose config has changed,
            and restarting them.

            FIXME: Given the above comment, this should be converted to a loop in which we modify
            the supervisor group behavior on config changes and selectively restart them as needed.
            For checking the supervisors in a non-blocking way, we can use Handle::is_finished().

            Suppose there's a config change. Situations:
            - Current agents stay as is, new agents are added: start these new agents, merge them with the current group.
            - Current agents stay as is, some agents are removed: get list of these agents (by key), stop and remove them from the current group.
            - Updated config for a certain agent(s) (type, name). Get (by key), stop, remove from the current group, start again with the new config and merge with the running group.

            The "merge" operation can only be done if the agents are of the same type! Supervisor<Running>. If they are not started we won't be able to merge them to the running group, as they are different types.
        */

        info!("Starting the supervisor group.");
        // Run all the agents in the supervisor group
        let running_supervisors = supervisor_group.run();

        // watch for supervisors restart requests
        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        AgentEvent::Stop => {
                            break running_supervisors.stop().into_iter().for_each(
                                |(agent_id, handles)| {
                                    for handle in handles {
                                        let agent_id = agent_id.clone();
                                        let agent_id1 = agent_id.clone(); // FIXME
                                        handle.join().map_or_else(
                                            |_err| {
                                                // let error: &dyn std::error::Error = &err;
                                                error!(
                                                    supervisor = agent_id.get(),
                                                    msg = "stopped with error",
                                                )
                                            },
                                            |_| {
                                                info!(
                                                    supervisor = agent_id1.get(),
                                                    msg = "stopped successfully"
                                                )
                                            },
                                        )
                                    }
                                },
                            );
                        }

                        AgentEvent::Restart(_agent_type) => {
                            // restart the corresponding supervisor
                            // TODO: remove agent from map, stop, run and reinsert it again
                        }
                    };
                }
                // spurious condvar wake up, loop should continue
            }
        }

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("SuperAgent finished");
        Ok(())
    }
}

fn load_agent_cfgs<Repo: AgentRepository>(
    agent_type_repository: &Repo,
    agent_cfgs: &SuperAgentConfig,
) -> Result<LocalRepository, AgentError> {
    let mut effective_agent_repository = LocalRepository::default();
    for (k, agent_cfg) in agent_cfgs.agents.iter() {
        let agent_type = agent_type_repository.get(&agent_cfg.agent_type)?;

        let contents = fs::read_to_string(&agent_cfg.values_file)?;
        let agent_config: SupervisorConfig = serde_yaml::from_str(&contents)?;

        let populated_agent = agent_type.clone().populate(agent_config)?;
        effective_agent_repository.store_with_key(k.get(), populated_agent)?;
    }
    Ok(effective_agent_repository)
}

#[cfg(test)]
mod tests {

    use crate::config::agent_type_registry::AgentRepository;

    use super::{supervisor_group::tests::new_sleep_supervisor_group, SupervisorGroupResolver};

    struct MockedSleepGroupResolver;
    impl<Repo> SupervisorGroupResolver<Repo> for MockedSleepGroupResolver
    where
        Repo: AgentRepository,
    {
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agent_repository: Repo,
        ) -> super::supervisor_group::SupervisorGroup<crate::supervisor::runner::Stopped> {
            new_sleep_supervisor_group(tx)
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let agent: Agent<LocalRepository, MockedSleepGroupResolver> = Agent::new_custom_resolver(MockedSleepGroupResolver, LocalRepository::default());
        let ctx: Context<Option<AgentEvent>> = Context::new();
      // stop all agents after 3 seconds
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(Duration::from_secs(3));
                ctx.cancel_all(Some(AgentEvent::Stop)).unwrap();
            }
        });
      assert!(agent.run(ctx).is_ok())
    }
}
