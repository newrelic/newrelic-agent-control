use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self, Sender};

use futures::executor::block_on;
use nix::unistd::gethostname;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use opamp_client::{capabilities, Client};
use opamp_client::{NotStartedClient, StartedClient};
use thiserror::Error;
use tracing::{error, info};

use crate::agent::defaults::{
    SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
};
use crate::agent::instance_id::{InstanceIDGetter, ULIDInstanceIDGetter};
use crate::agent::EffectiveAgentsError::{EffectiveAgentExists, EffectiveAgentNotFound};
use crate::config::agent_type::agent_types::FinalAgent;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPHttpBuilder};
use crate::{
    agent::supervisor_group::SupervisorGroup,
    command::{stream::Event, EventLogger, StdEventReceiver},
    config::agent_configs::{AgentID, SuperAgentConfig},
    context::Context,
    supervisor::runner::Stopped,
};

use self::error::AgentError;

pub mod callbacks;
pub mod defaults;
pub mod effective_agents_assembler;
pub mod error;
pub mod instance_id;
pub mod supervisor_group;

#[derive(Clone)]
pub enum AgentEvent {
    // this should be a list of agentTypes
    Restart(AgentID),
    // stop all supervisors
    Stop,
}

pub trait SupervisorGroupResolver<OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agents: &EffectiveAgents,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError>;
}

pub struct Agent<OpAMPBuilder = OpAMPHttpBuilder, ID = ULIDInstanceIDGetter, R = SuperAgentConfig>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    R: SupervisorGroupResolver<OpAMPBuilder, ID>,
{
    resolver: R,
    instance_id_getter: ID,
    effective_agents: EffectiveAgents,
    opamp_client_builder: Option<OpAMPBuilder>,
}

impl<OpAMPBuilder, ID> SupervisorGroupResolver<OpAMPBuilder, ID> for SuperAgentConfig
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn retrieve_group(
        &self,
        tx: Sender<Event>,
        effective_agents: &EffectiveAgents,
        opamp_client_builder: &Option<OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<SupervisorGroup<OpAMPBuilder::Client, Stopped>, AgentError> {
        Ok(SupervisorGroup::<OpAMPBuilder::Client, Stopped>::new(
            effective_agents,
            tx,
            self.clone(),
            opamp_client_builder.as_ref(),
            instance_id_getter,
        )?)
    }
}

impl<OpAMPBuilder, ID> Agent<OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn new(
        cfg: SuperAgentConfig,
        effective_agents: EffectiveAgents,
        opamp_client_builder: Option<OpAMPBuilder>,
        instance_id_getter: ID,
    ) -> Self {
        Self {
            resolver: cfg,
            instance_id_getter,
            effective_agents,
            opamp_client_builder,
        }
    }

    #[cfg(test)]
    pub fn new_custom<R>(
        resolver: R,
        instance_id_getter: ID,
        effective_agents: EffectiveAgents,
        opamp_client_builder: Option<OpAMPBuilder>,
    ) -> Agent<OpAMPBuilder, ID, R>
    where
        R: SupervisorGroupResolver<OpAMPBuilder, ID>,
    {
        Agent {
            resolver,
            effective_agents,
            opamp_client_builder,
            instance_id_getter,
        }
    }
}

impl<OpAMPBuilder, R, ID> Agent<OpAMPBuilder, ID, R>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    R: SupervisorGroupResolver<OpAMPBuilder, ID>,
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
                let opamp_client_handle = block_on(opamp_client.start())?;

                let health = opamp_client::opamp::proto::AgentHealth {
                    healthy: true,
                    last_error: "".to_string(),
                    start_time_unix_nano: 0,
                };
                block_on(opamp_client_handle.set_health(health))?;
                Some(opamp_client_handle)
            }
            None => None,
        };

        info!("Starting the supervisor group.");
        let supervisor_group = self.resolver.retrieve_group(
            tx,
            &self.effective_agents,
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
            info!("Stopping and setting to unhealthy the OpAMP Client");
            let health = opamp_client::opamp::proto::AgentHealth {
                healthy: false,
                last_error: "".to_string(),
                start_time_unix_nano: 0,
            };
            block_on(handle.set_health(health))?;
            block_on(handle.stop())?;
        }

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("SuperAgent finished");
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct EffectiveAgents(HashMap<String, FinalAgent>);

#[derive(Error, Debug)]
pub enum EffectiveAgentsError {
    #[error("effective agent `{0}` not found")]
    EffectiveAgentNotFound(String),
    #[error("effective agent `{0}` already exists")]
    EffectiveAgentExists(String),
}

impl EffectiveAgents {
    pub fn get(&self, agent_id: &AgentID) -> Result<&FinalAgent, EffectiveAgentsError> {
        match self.0.get(agent_id.get().as_str()) {
            None => Err(EffectiveAgentNotFound(agent_id.get())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(
        &mut self,
        agent_id: &AgentID,
        agent: FinalAgent,
    ) -> Result<(), EffectiveAgentsError> {
        if self.get(agent_id).is_ok() {
            return Err(EffectiveAgentExists(agent_id.get()));
        }
        self.0.insert(agent_id.get().to_string(), agent);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::agent::defaults::{
        SUPER_AGENT_ID, SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION,
    };
    use crate::agent::error::AgentError;
    use crate::agent::instance_id::test::MockInstanceIDGetterMock;
    use crate::agent::instance_id::InstanceIDGetter;
    use crate::agent::{Agent, AgentEvent, EffectiveAgents};
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

    impl<OpAMPBuilder, ID> SupervisorGroupResolver<OpAMPBuilder, ID> for MockedSleepGroupResolver
    where
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        fn retrieve_group(
            &self,
            tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
            _effective_agents: &EffectiveAgents,
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
                        .times(2)
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
            MockOpAMPClientBuilderMock,
            MockInstanceIDGetterMock,
            MockedSleepGroupResolver,
        > = Agent::new_custom(
            MockedSleepGroupResolver,
            instance_id_getter,
            EffectiveAgents::default(),
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
