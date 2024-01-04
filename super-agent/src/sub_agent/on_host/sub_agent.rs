use std::thread::JoinHandle;

use opamp_client;
use opamp_client::StartedClient;
use tracing::debug;

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::super_agent_configs::AgentID;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::SubAgentError;

use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentCallbacks};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    opamp_client: Option<C>,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> NotStartedSubAgentOnHost<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<NotStartedSupervisorOnHost>,
        opamp_client: Option<C>,
    ) -> Result<Self, SubAgentError> {
        Ok(NotStartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
        })
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C> NotStartedSubAgent for NotStartedSubAgentOnHost<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    type StartedSubAgent = StartedSubAgentOnHost<C>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| {
                debug!("Running supervisor {} for {}", s.config.bin, self.agent_id);
                s.run()
            })
            .collect::<Result<Vec<_>, _>>()?;

        let started_sub_agent = StartedSubAgentOnHost {
            opamp_client: self.opamp_client,
            supervisors: started_supervisors,
            agent_id: self.agent_id,
        };

        Ok(started_sub_agent)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentOnHost<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> StartedSubAgent for StartedSubAgentOnHost<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();
        stop_opamp_client(self.opamp_client, &self.agent_id)?;
        Ok(stopped_supervisors)
    }
}
