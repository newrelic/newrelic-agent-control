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

use self::{
    error::AgentError,
    opamp_builder::{OpAMPClientBuilder, OpAMPHttpBuilder},
};

pub mod callbacks;
pub mod error;
pub(super) mod opamp_builder;
pub mod supervisor_group;

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver<Repo, OpAMPBuilder>
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError>;
}

impl<Repo, OpAMPBuilder> SupervisorGroupResolver<Repo, OpAMPBuilder> for SuperAgentConfig
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        SupervisorGroup::<OpAMPBuilder::Client, Stopped>::new(
            tx,
            self,
            effective_agent_repository,
            opamp_client_builder,
        )
    }
}

pub struct Agent<
    Repo,
    EffectiveRepo = LocalRepository,
    OpAMPBuilder = OpAMPHttpBuilder,
    R = SuperAgentConfig,
> where
    Repo: AgentRepository,
    EffectiveRepo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder>,
{
    resolver: R,
    agent_type_repository: Repo,
    effective_agent_repository: EffectiveRepo,
    opamp_client_builder: OpAMPBuilder,
}

impl<Repo> Agent<Repo>
where
    Repo: AgentRepository + Clone,
{
    pub fn new(cfg_path: &Path, agent_type_repository: Repo) -> Result<Self, AgentError> {
        let cfg = Resolver::retrieve_config(cfg_path)?;

        let effective_agent_repository = load_agent_cfgs(&agent_type_repository, &cfg)?;

        let opamp_client_builder = OpAMPHttpBuilder::new(cfg.opamp.clone());

        Ok(Self {
            resolver: cfg,
            agent_type_repository,
            effective_agent_repository,
            opamp_client_builder,
        })
    }

    #[cfg(test)]
    pub fn new_custom_resolver<
        R,
        EffectiveRepo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
    >(
        resolver: R,
        local_repo: Repo,
        effective_repo: EffectiveRepo,
        opamp_client_builder: OpAMPBuilder,
    ) -> Agent<Repo, EffectiveRepo, OpAMPBuilder, R>
    where
        R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder>,
    {
        Agent {
            resolver,
            agent_type_repository: local_repo,
            effective_agent_repository: effective_repo,
            opamp_client_builder,
        }
    }
}

impl<Repo, EffectiveRepo, OpAMPBuilder, R> Agent<Repo, EffectiveRepo, OpAMPBuilder, R>
where
    OpAMPBuilder: OpAMPClientBuilder,
    R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder>,
    Repo: AgentRepository,
    EffectiveRepo: AgentRepository,
{
    pub fn run(self, ctx: Context<Option<AgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        let supervisor_group = self.resolver.retrieve_group(
            tx,
            self.effective_agent_repository,
            self.opamp_client_builder,
        )?;
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
    use crate::agent::error::AgentError;
    use crate::agent::opamp_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::agent::{Agent, AgentEvent};
    use crate::config::agent_type_registry::{AgentRepository, LocalRepository};
    use crate::context::Context;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::opamp_builder::OpAMPClientBuilder;
    use super::{supervisor_group::tests::new_sleep_supervisor_group, SupervisorGroupResolver};

    struct MockedSleepGroupResolver;
    impl<Repo, OpAMPBuilder> SupervisorGroupResolver<Repo, OpAMPBuilder> for MockedSleepGroupResolver
    where
        Repo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
    {
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agent_repository: Repo,
            opamp_client_builder: OpAMPBuilder,
        ) -> Result<
            super::supervisor_group::SupervisorGroup<
                OpAMPBuilder::Client,
                crate::supervisor::runner::Stopped,
            >,
            AgentError,
        > {
            new_sleep_supervisor_group(tx, opamp_client_builder)
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        // two agents in the supervisor group
        opamp_builder.expect_build().times(2).returning(|_| {
            let mut opamp_client = MockOpAMPClientMock::new();
            opamp_client.expect_start().once().returning(|| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.expect_stop().once().returning(|| Ok(()));
                Ok(started_client)
            });

            Ok(opamp_client)
        });

        let agent: Agent<
            LocalRepository,
            LocalRepository,
            MockOpAMPClientBuilderMock,
            MockedSleepGroupResolver,
        > = Agent::new_custom_resolver(
            MockedSleepGroupResolver,
            LocalRepository::default(),
            LocalRepository::default(),
            opamp_builder,
        );
        let ctx = Context::new();
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
