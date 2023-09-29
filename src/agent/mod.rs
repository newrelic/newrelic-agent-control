use std::collections::HashMap;
use std::string::ToString;
use std::{
    fs,
    sync::mpsc::{self, Sender},
};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use opamp_client::{capabilities, OpAMPClient, OpAMPClientHandle};
use tracing::{error, info};

use crate::agent::instance_id::{InstanceIDGetter, ULIDInstanceIDGetter};
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{stream::Event, EventLogger, StdEventReceiver},
    config::{
        agent_configs::{AgentID, SuperAgentConfig},
        agent_type_registry::{AgentRepository, LocalRepository},
        supervisor_config::SupervisorConfig,
    },
    context::Context,
    supervisor::runner::Stopped,
};

use self::error::AgentError;

pub mod callbacks;
pub mod error;
pub mod instance_id;
pub mod supervisor_group;

const SUPER_AGENT_ID: &str = "super-agent";
const SUPER_AGENT_TYPE: &str = "com.newrelic.meta_agent";
const SUPER_AGENT_NAMESPACE: &str = "newrelic";
const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: &Repo,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError>;
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
    opamp_client_builder: Option<OpAMPBuilder>,
}

impl<Repo, OpAMPBuilder, ID> SupervisorGroupResolver<Repo, OpAMPBuilder, ID> for SuperAgentConfig
where
    Repo: AgentRepository,
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agent_repository: &Repo,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        Ok(SupervisorGroup::<OpAMPBuilder::Client, Stopped>::new(
            effective_agent_repository,
            tx,
            self.clone(),
            opamp_client_builder.as_ref(),
            instance_id_getter,
        )?)
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
        opamp_client_builder: Option<OpAMPBuilder>,
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
        opamp_client_builder: Option<OpAMPBuilder>,
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

        // build and start the Agent's OpAMP client if a builder is provided
        let opamp_client_handle = match self.opamp_client_builder {
            Some(ref builder) => {
                info!("Starting superagent's OpAMP Client.");
                let opamp_client = builder.build(StartSettings {
                    instance_id: self.instance_id_getter.get(SUPER_AGENT_ID.to_string()),
                    capabilities: capabilities!(AgentCapabilities::ReportsHealth),
                    agent_description: AgentDescription {
                        identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                            ("service.name".to_string(), SUPER_AGENT_TYPE.into()),
                            (
                                "service.namespace".to_string(),
                                SUPER_AGENT_NAMESPACE.into(),
                            ),
                            ("service.version".to_string(), SUPER_AGENT_VERSION.into()),
                        ]),
                        non_identifying_attributes: HashMap::from([(
                            "host.name".to_string(),
                            gethostname()
                                .unwrap_or_default()
                                .into_string()
                                .unwrap()
                                .into(),
                        )]),
                    },
                })?;
                let mut opamp_client_handle = block_on(opamp_client.start()).unwrap();

                let health = opamp_client::opamp::proto::AgentHealth {
                    healthy: true,
                    last_error: "".to_string(),
                    start_time_unix_nano: 0,
                };
                block_on(opamp_client_handle.set_health(&health)).unwrap();
                Some(opamp_client_handle)
            }
            None => None,
        };

        info!("Starting the supervisor group.");
        let supervisor_group = self.resolver.retrieve_group(
            tx,
            &self.effective_agent_repository,
            &self.opamp_client_builder,
            &self.instance_id_getter,
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

        // Run all the agents in the supervisor group
        let running_supervisors = supervisor_group.run()?;

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

        if let Some(handle) = opamp_client_handle {
            info!("Stopping OpAMP Client");
            block_on(handle.stop()).unwrap();
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
        let agent_type = agent_type_repository.get(&agent_cfg.agent_type.to_string())?;

        let contents = fs::read_to_string(&agent_cfg.values_file)?;
        let agent_config: SupervisorConfig = serde_yaml::from_str(&contents)?;

        let populated_agent = agent_type.clone().template_with(agent_config)?;
        effective_agent_repository.store_with_key(k.get(), populated_agent)?;
    }
    Ok(effective_agent_repository)
}

#[cfg(test)]
mod tests {
    use crate::agent::error::AgentError;
    use crate::agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::agent::instance_id::InstanceIDGetter;
    use crate::agent::{
        Agent, AgentEvent, SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE,
        SUPER_AGENT_VERSION,
    };
    use crate::config::agent_type_registry::{AgentRepository, LocalRepository};
    use crate::context::Context;
    use crate::opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock};
    use crate::opamp::client_builder::OpAMPClientBuilder;
    use mockall::predicate;
    use nix::unistd::gethostname;
    use opamp_client::capabilities;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::operation::capabilities::Capabilities;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::{supervisor_group::tests::new_sleep_supervisor_group, SupervisorGroupResolver};

    struct MockedSleepGroupResolver;

    impl<Repo, OpAMPBuilder, ID> SupervisorGroupResolver<Repo, OpAMPBuilder, ID>
        for MockedSleepGroupResolver
    where
        Repo: AgentRepository,
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agent_repository: &Repo,
            opamp_client_builder: &Option<OpAMPBuilder>,
            _instance_id_getter: &ID,
        ) -> Result<
            super::supervisor_group::SupervisorGroup<
                OpAMPBuilder::Client,
                crate::supervisor::runner::Stopped,
            >,
            AgentError,
        > {
            new_sleep_supervisor_group(tx, Some(opamp_client_builder.as_ref().unwrap()))
        }
    }

    #[test]
    fn run_and_stop_supervisors() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let start_settings = StartSettings {
            instance_id: SUPER_AGENT_ID.to_string(),
            capabilities: capabilities!(AgentCapabilities::ReportsHealth),
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    ("service.name".to_string(), SUPER_AGENT_TYPE.into()),
                    (
                        "service.namespace".to_string(),
                        SUPER_AGENT_NAMESPACE.into(),
                    ),
                    ("service.version".to_string(), SUPER_AGENT_VERSION.into()),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "host.name".to_string(),
                    gethostname()
                        .unwrap_or_default()
                        .into_string()
                        .unwrap()
                        .into(),
                )]),
            },
        };

        opamp_builder
            .expect_build()
            .with(predicate::eq(start_settings))
            .times(1)
            .returning(|_| {
                let mut opamp_client = MockOpAMPClientMock::new();
                opamp_client.expect_start().with().once().returning(|| {
                    let mut started_client = MockOpAMPClientMock::new();
                    started_client.expect_stop().once().returning(|| Ok(()));
                    started_client
                        .expect_set_health()
                        .once()
                        .returning(|_| Ok(()));
                    Ok(started_client)
                });

                Ok(opamp_client)
            });

        let start_settings = StartSettings {
            instance_id: "testing".to_string(),
            capabilities: Capabilities::default(),
            ..Default::default()
        };

        opamp_builder
            .expect_build()
            .with(predicate::eq(start_settings))
            .times(2)
            .returning(|_| {
                let mut opamp_client = MockOpAMPClientMock::new();
                opamp_client.expect_start().with().once().returning(|| {
                    let mut started_client = MockOpAMPClientMock::new();
                    started_client.expect_stop().once().returning(|| Ok(()));
                    started_client
                        .expect_set_health()
                        .times(2)
                        .returning(|_| Ok(()));
                    Ok(started_client)
                });

                Ok(opamp_client)
            });

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
            Some(opamp_builder),
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
