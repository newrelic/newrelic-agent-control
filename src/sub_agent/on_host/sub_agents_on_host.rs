use crate::config::super_agent_configs::AgentID;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::sub_agent::on_host::sub_agent_on_host::{
    NotStartedSubAgentOnHost, StartedSubAgentOnHost,
};
use crate::sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::instance_id::InstanceIDGetter;
use opamp_client::StartedClient;
use std::collections::HashMap;
use std::thread::JoinHandle;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgents On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentsOnHost<'a, OpAMPBuilder, ID>(
    HashMap<AgentID, NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>>,
)
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter;

impl<'a, OpAMPBuilder, ID> NotStartedSubAgentsOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub(super) fn add(&mut self, sub_agent: NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>) {
        self.0.insert(sub_agent.agent_id().clone(), sub_agent);
    }

    pub fn run(self) -> Result<StartedSubAgentsOnHost<OpAMPBuilder::Client>, SubAgentError> {
        let mut started_sub_agents = StartedSubAgentsOnHost::default();
        let result: Result<(), SubAgentError> =
            self.0.into_iter().try_for_each(|(_agent_id, agent)| {
                let started_sub_agent = agent.run()?;
                started_sub_agents.add(started_sub_agent)?;
                Ok(())
            });

        match result {
            Err(e) => Err(e),
            _ => Ok(started_sub_agents),
        }
    }
}

impl<'a, OpAMPBuilder, ID> Default for NotStartedSubAgentsOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    fn default() -> Self {
        NotStartedSubAgentsOnHost(HashMap::new())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgents On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentsOnHost<C>
where
    C: StartedClient,
{
    agents: HashMap<AgentID, StartedSubAgentOnHost<C>>,
}

impl<C> StartedSubAgentsOnHost<C>
where
    C: StartedClient,
{
    pub fn add(&mut self, sub_agent: StartedSubAgentOnHost<C>) -> Result<(), SubAgentError> {
        if self.agents.contains_key(sub_agent.agent_id()) {
            return Err(SubAgentError::AgentAlreadyExists(
                sub_agent.agent_id().to_string(),
            ));
        }
        self.agents.insert(sub_agent.agent_id().clone(), sub_agent);
        Ok(())
    }

    pub fn remove(
        &mut self,
        agent_id: &AgentID,
    ) -> Result<StartedSubAgentOnHost<C>, SubAgentError> {
        if !self.agents.contains_key(agent_id) {
            return Err(SubAgentError::AgentNotFound(agent_id.to_string()));
        }
        Ok(self.agents.remove(agent_id).unwrap())
    }

    pub fn stop(self) -> Result<HashMap<AgentID, Vec<JoinHandle<()>>>, SubAgentError> {
        let mut stopped_agents_handles: HashMap<AgentID, Vec<JoinHandle<()>>> = HashMap::new();

        let result: Result<(), SubAgentError> =
            self.agents.into_iter().try_for_each(|(t, agent)| {
                let handle = agent.stop()?;
                stopped_agents_handles.insert(t.clone(), handle);
                Ok(())
            });

        match result {
            Err(e) => Err(e),
            _ => Ok(stopped_agents_handles),
        }
    }
}

impl<C> Default for StartedSubAgentsOnHost<C>
where
    C: StartedClient,
{
    fn default() -> Self {
        StartedSubAgentsOnHost {
            agents: HashMap::new(),
        }
    }
}
