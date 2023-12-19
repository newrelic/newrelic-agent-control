use std::thread::JoinHandle;

use opamp_client;
use opamp_client::StartedClient;
use tracing::debug;

use super::supervisor::command_supervisor::SupervisorOnHost;
use crate::config::super_agent_configs::AgentID;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::SubAgentError;

use super::supervisor::command_supervisor;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentCallbacks};

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started Sub Agents
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted;
pub struct Started;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentOnHost<C, S, V>
where
    C: StartedClient<SubAgentCallbacks>,
{
    opamp_client: Option<C>,
    supervisors: Vec<SupervisorOnHost<V>>,
    agent_id: AgentID,
    state: S,
}

impl<C, S, V> SubAgentOnHost<C, S, V>
where
    C: StartedClient<SubAgentCallbacks>,
{
    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C> SubAgentOnHost<C, NotStarted, command_supervisor::NotStarted>
where
    C: StartedClient<SubAgentCallbacks>,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<SupervisorOnHost<command_supervisor::NotStarted>>,
        opamp_client: Option<C>,
    ) -> SubAgentOnHost<C, NotStarted, command_supervisor::NotStarted> {
        SubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
            state: NotStarted,
        }
    }
}

impl<C> NotStartedSubAgent for SubAgentOnHost<C, NotStarted, command_supervisor::NotStarted>
where
    C: StartedClient<SubAgentCallbacks>,
{
    type StartedSubAgent = SubAgentOnHost<C, Started, command_supervisor::Started>;

    fn run(self) -> Result<SubAgentOnHost<C, Started, command_supervisor::Started>, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| {
                debug!("Running supervisor {} for {}", s.id(), self.agent_id);
                s.run()
            })
            .collect::<Result<Vec<_>, _>>()?;

        let started_sub_agent = SubAgentOnHost {
            opamp_client: self.opamp_client,
            supervisors: started_supervisors,
            agent_id: self.agent_id,
            state: Started,
        };

        Ok(started_sub_agent)
    }
}

impl<C> StartedSubAgent for SubAgentOnHost<C, Started, command_supervisor::Started>
where
    C: StartedClient<SubAgentCallbacks>,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();
        stop_opamp_client(self.opamp_client, &self.agent_id)?;
        Ok(stopped_supervisors)
    }
}
