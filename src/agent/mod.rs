use std::string::ToString;
use std::{
    fs,
    sync::mpsc::{self, Sender},
};

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{capabilities, OpAMPClient, OpAMPClientHandle};
use tracing::{error, info};

use crate::agent::instance_id::{InstanceIDGetter, ULIDInstanceIDGetter};
use crate::agent::supervisor_group::SupervisorGroupBuilder;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::supervisor::runner::Stopped;
use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{stream::Event, EventLogger, StdEventReceiver},
    config::{
        agent_configs::{AgentID, SuperAgentConfig},
        agent_type_registry::{AgentRepository, LocalRepository},
        supervisor_config::SupervisorConfig,
    },
    context::Context,
};

use self::error::AgentError;
use self::supervisor_group::{StartedSupervisorGroup, SupervisorOpAMPGroup};

pub mod callbacks;
pub mod error;
pub mod instance_id;
pub mod supervisor_group;

const SUPER_AGENT_ID: &str = "super-agent";

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver<Repo, OpAMPBuilder, ID>
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    type SupervisorWithOpAMP: SupervisorGroup;

    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
        instance_id_getter: ID,
    ) -> Result<Self::SupervisorWithOpAMP, AgentError>;
}

pub struct Agent<
    Repo,
    OpAMPBuilder = OpAMPHttpBuilder,
    ID = ULIDInstanceIDGetter,
    EffectiveRepo = LocalRepository,
    R = SuperAgentConfig,
> where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    EffectiveRepo: AgentRepository,
    R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder, ID>,
{
    resolver: R,
    agent_type_repository: Repo,
    instance_id_getter: ID,
    effective_agent_repository: EffectiveRepo,
    opamp_client_builder: OpAMPBuilder,
}

impl<Repo, OpAMPBuilder, ID> SupervisorGroupResolver<Repo, OpAMPBuilder, ID> for SuperAgentConfig
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    type SupervisorWithOpAMP = SupervisorOpAMPGroup<OpAMPBuilder::Client, Stopped>;

    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
        instance_id_getter: ID,
    ) -> Result<Self::SupervisorWithOpAMP, AgentError> {
        let builder = SupervisorGroupBuilder {
            tx,
            cfg: self.clone(),
            effective_agent_repository,
            opamp_builder: opamp_client_builder,
            instance_id_getter,
        };
        Ok(builder.build()?)
    }
}

impl<Repo, OpAMPBuilder, ID> Agent<Repo, OpAMPBuilder, ID>
where
    Repo: AgentRepository + Clone,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn new(
        cfg: SuperAgentConfig,
        agent_type_repository: Repo,
        opamp_client_builder: OpAMPBuilder,
        instance_id_getter: ID,
    ) -> Result<Self, AgentError> {
        let effective_agent_repository = load_agent_cfgs(&agent_type_repository, &cfg)?;

        Ok(Self {
            resolver: cfg,
            agent_type_repository,
            instance_id_getter,
            effective_agent_repository,
            opamp_client_builder,
        })
    }

    #[cfg(test)]
    pub fn new_custom<R, EffectiveRepo: AgentRepository>(
        resolver: R,
        local_repo: Repo,
        instance_id_getter: ID,
        effective_repo: EffectiveRepo,
        opamp_client_builder: OpAMPBuilder,
    ) -> Agent<Repo, OpAMPBuilder, ID, EffectiveRepo, R>
    where
        R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder, ID>,
    {
        Agent {
            resolver,
            agent_type_repository: local_repo,
            effective_agent_repository: effective_repo,
            opamp_client_builder,
            instance_id_getter,
        }
    }
}

impl<Repo, OpAMPBuilder, EffectiveRepo, R, ID> Agent<Repo, OpAMPBuilder, ID, EffectiveRepo, R>
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    EffectiveRepo: AgentRepository,
    R: SupervisorGroupResolver<EffectiveRepo, OpAMPBuilder, ID>,
{
    pub fn run(self, ctx: Context<Option<AgentEvent>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        info!("Starting superagent's OpAMP Client.");
        // Run all the agents in the supervisor group
        let opamp_client = self.opamp_client_builder.build(StartSettings {
            instance_id: self.instance_id_getter.get(SUPER_AGENT_ID.to_string()),
            capabilities: capabilities!(AgentCapabilities::ReportsHealth),
        })?;
        let mut opamp_client_handle = block_on(opamp_client.start()).unwrap();

        let health = opamp_client::opamp::proto::AgentHealth {
            healthy: true,
            last_error: "".to_string(),
            start_time_unix_nano: 0,
        };
        block_on(opamp_client_handle.set_health(&health)).unwrap();

        info!("Starting the supervisor group.");
        let supervisors = self.resolver.retrieve_group(
            tx,
            self.effective_agent_repository,
            self.opamp_client_builder,
            self.instance_id_getter,
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

        let running_supervisors = supervisors.run()?;

        {
            loop {
                // blocking wait until context is woken up
                if let Some(event) = ctx.wait_condvar().unwrap() {
                    match event {
                        AgentEvent::Stop => {
                            break running_supervisors.stop()?.into_iter().for_each(
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

        info!("Stopping OpAMP Client");
        let _ = opamp_client_handle.stop();

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
    use crate::agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::agent::instance_id::InstanceIDGetter;
    use crate::agent::{Agent, AgentEvent, SUPER_AGENT_ID};
    use crate::config::agent_configs::AgentID;
    use crate::config::agent_type_registry::{AgentRepository, LocalRepository};
    use crate::context::Context;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::opamp::client_builder::OpAMPClientBuilder;
    use mockall::predicate;
    use opamp_client::capabilities;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::operation::settings::StartSettings;
    use std::collections::HashMap;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::supervisor_group::tests::{MockStartedSupervisorGroupMock, MockSupervisorGroupMock};
    use super::SupervisorGroupResolver;

    struct MockedSleepGroupResolver;
    impl<Repo, OpAMPBuilder, ID> SupervisorGroupResolver<Repo, OpAMPBuilder, ID>
        for MockedSleepGroupResolver
    where
        Repo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        type SupervisorWithOpAMP = MockSupervisorGroupMock;
        fn retrieve_group(
            &self,
            _tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agent_repository: Repo,
            _opamp_client_builder: OpAMPBuilder,
            _instance_id_getter: ID,
        ) -> Result<Self::SupervisorWithOpAMP, AgentError> {
            let mut mock_group = MockSupervisorGroupMock::new();
            mock_group.expect_run().once().returning(|| {
                let mut started_group = MockStartedSupervisorGroupMock::new();
                started_group.expect_stop().once().returning(|| {
                    let handle = spawn(|| ());
                    Ok(HashMap::from([(AgentID("test".to_string()), vec![handle])]))
                });
                Ok(started_group)
            });
            Ok(mock_group)
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let start_settings = StartSettings {
            instance_id: SUPER_AGENT_ID.to_string(),
            capabilities: capabilities!(AgentCapabilities::ReportsHealth),
        };

        opamp_builder
            .expect_build()
            .with(predicate::eq(start_settings))
            .times(1)
            .returning(|_| {
                let mut opamp_client = MockOpAMPClientMock::new();
                opamp_client.expect_start().with().once().returning(|| {
                    let mut started_client = MockOpAMPClientMock::new();
                    started_client
                        .expect_set_health()
                        .once()
                        .returning(|_| Ok(()));
                    Ok(started_client)
                });

                Ok(opamp_client)
            });

        // let start_settings = StartSettings {
        //     instance_id: "testing".to_string(),
        //     capabilities: Capabilities::default(),
        // };

        // opamp_builder
        //     .expect_build()
        //     .with(predicate::eq(start_settings))
        //     .times(2)
        //     .returning(|_| {
        //         let mut opamp_client = MockOpAMPClientMock::new();
        //         opamp_client.expect_start().with().once().returning(|| {
        //             let mut started_client = MockOpAMPClientMock::new();
        //             started_client.expect_stop().once().returning(|| Ok(()));
        //             started_client.expect_set_health().never();
        //             Ok(started_client)
        //         });
        //
        //         Ok(opamp_client)
        //     });

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter
            .expect_get()
            .times(1)
            .returning(|name| name);

        // two agents in the supervisor group
        let agent: Agent<
            LocalRepository,
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            LocalRepository,
            MockedSleepGroupResolver,
        > = Agent::new_custom(
            MockedSleepGroupResolver,
            LocalRepository::default(),
            instance_id_getter,
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
