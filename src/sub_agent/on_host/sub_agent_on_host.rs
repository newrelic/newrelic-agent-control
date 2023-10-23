use std::thread::JoinHandle;

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::StartedClient;
use tracing::info;

use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::on_host::factory::build_opamp_and_start_client;
use crate::sub_agent::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentError};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::utils::time::get_sys_time_nano;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub(super) struct NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
}

impl<'a, OpAMPBuilder, ID> NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<NotStartedSupervisorOnHost>,
        opamp_builder: Option<&'a OpAMPBuilder>,
        instance_id_getter: &'a ID,
        agent_type: AgentTypeFQN,
    ) -> Self {
        NotStartedSubAgentOnHost {
            opamp_builder,
            instance_id_getter,
            supervisors,
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

impl<'a, OpAMPBuilder, ID> NotStartedSubAgent for NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    type StartedSubAgent = StartedSubAgentOnHost<OpAMPBuilder::Client>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let agent_id = self.agent_id.clone();
        let started_opamp_client = self.run_opamp_client()?;
        let mut supervisors = Vec::new();
        for supervisor in self.supervisors {
            supervisors.push(supervisor.run()?);
        }
        Ok(StartedSubAgentOnHost::new(
            agent_id,
            started_opamp_client,
            supervisors,
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub(super) struct StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    pub fn new(
        agent_id: AgentID,
        opamp_client: Option<C>,
        supervisors: Vec<StartedSupervisorOnHost>,
    ) -> Self
    where
        C: StartedClient,
    {
        StartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
        }
    }
}

impl<C> StartedSubAgent for StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    type S = JoinHandle<()>;

    fn stop(self) -> Result<Vec<Self::S>, SubAgentError> {
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

        let mut stopped_runners = Vec::new();
        for supervisors in self.supervisors {
            stopped_runners.push(supervisors.stop());
        }
        Ok(stopped_runners)
    }
}
