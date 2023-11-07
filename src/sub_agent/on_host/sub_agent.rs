use std::thread::JoinHandle;

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::StartedClient;
use tracing::info;

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::super_agent_configs::AgentID;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::utils::time::get_sys_time_nano;

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    opamp_client: Option<C>,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> NotStartedSubAgentOnHost<C>
where
    C: StartedClient,
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
    C: StartedClient,
{
    type RunningSubAgent = StartedSubAgentOnHost<C>;

    fn run(self) -> Result<Self::RunningSubAgent, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| s.run())
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
    C: StartedClient,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> StartedSubAgent for StartedSubAgentOnHost<C>
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

        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();

        Ok(stopped_supervisors)
    }
}
