use std::thread::JoinHandle;

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::StartedClient;
use tracing::info;

use crate::config::agent_configs::{AgentID, AgentTypeFQN};
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::k8s::factory::build_opamp_and_start_client;
use crate::sub_agent::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentError};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::utils::time::get_sys_time_nano;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentK8S<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
}

impl<'a, OpAMPBuilder, ID> NotStartedSubAgentK8S<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn new(
        agent_id: AgentID,
        opamp_builder: Option<&'a OpAMPBuilder>,
        instance_id_getter: &'a ID,
        agent_type: AgentTypeFQN,
    ) -> Self {
        NotStartedSubAgentK8S {
            opamp_builder,
            instance_id_getter,
            agent_id,
            agent_type,
        }
    }

    fn run_opamp_client(&self) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError> {
        build_opamp_and_start_client(
            self.opamp_builder,
            self.instance_id_getter,
            self.agent_id.clone(),
            &self.agent_type,
        )
    }
}

impl<'a, OpAMPBuilder, ID> NotStartedSubAgent for NotStartedSubAgentK8S<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    type StartedSubAgent = StartedSubAgentK8S<OpAMPBuilder::Client>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let agent_id = self.agent_id.clone();
        let started_opamp_client = self.run_opamp_client()?;

        Ok(StartedSubAgentK8S::new(agent_id, started_opamp_client))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentK8S<C>
where
    C: StartedClient,
{
    opamp_client: Option<C>,
    agent_id: AgentID,
}

impl<C> StartedSubAgentK8S<C>
where
    C: StartedClient,
{
    pub fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self
    where
        C: StartedClient,
    {
        StartedSubAgentK8S {
            opamp_client,
            agent_id,
        }
    }
}

impl<C> StartedSubAgent for StartedSubAgentK8S<C>
where
    C: StartedClient,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let _client = match self.opamp_client {
            Some(client) => {
                info!(
                    "Stopping OpAMP client for supervised agent type: {}",
                    self.agent_id
                );
                // set OpAMP health
                block_on(client.set_health(AgentHealth {
                    healthy: false,
                    start_time_unix_nano: get_sys_time_nano()?,
                    last_error: "".to_string(),
                }))?;

                Some(block_on(client.stop())?)
            }
            None => None,
        };

        let stopped_runners = Vec::default();

        Ok(stopped_runners)
    }
}
