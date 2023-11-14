use std::collections::HashMap;
use std::thread::JoinHandle;

use nix::unistd::gethostname;
use opamp_client;

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::opamp;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::SuperAgentEvent;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>
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

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    fn run_opamp_client(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError> {
        let opamp_start_settings = opamp::start_settings(
            self.instance_id_getter.get(&self.agent_id),
            &self.agent_type,
            HashMap::from([("host.name".to_string(), get_hostname().into())]),
        );
        opamp::start_client(
            ctx,
            self.opamp_builder,
            self.agent_id.clone(),
            opamp_start_settings,
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
        let started_opamp_client = self.run_opamp_client(Context::new())?;
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
pub struct StartedSubAgentOnHost<C>
where
    C: opamp_client::StartedClient,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> StartedSubAgentOnHost<C>
where
    C: opamp_client::StartedClient,
{
    pub fn new(
        agent_id: AgentID,
        opamp_client: Option<C>,
        supervisors: Vec<StartedSupervisorOnHost>,
    ) -> Self
    where
        C: opamp_client::StartedClient,
    {
        StartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
        }
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C> StartedSubAgent for StartedSubAgentOnHost<C>
where
    C: opamp_client::StartedClient,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        opamp::stop_client(self.opamp_client, self.agent_id)?;

        let mut stopped_runners = Vec::new();
        for supervisors in self.supervisors {
            stopped_runners.push(supervisors.stop());
        }
        Ok(stopped_runners)
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}
