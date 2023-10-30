use std::collections::HashMap;

use crate::config::super_agent_configs::AgentID;

use tracing::{error, info};

use super::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent};

pub(crate) struct NotStartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: NotStartedSubAgent;

impl<S> From<HashMap<AgentID, S>> for NotStartedSubAgents<S>
where
    S: NotStartedSubAgent,
{
    fn from(value: HashMap<AgentID, S>) -> Self {
        Self(value)
    }
}

impl<S> NotStartedSubAgents<S>
where
    S: NotStartedSubAgent,
{
    pub fn new(value: HashMap<AgentID, S>) -> Self {
        Self(value)
    }
}

impl<S> NotStartedSubAgents<S>
where
    S: NotStartedSubAgent,
{
    pub(crate) fn run(self) -> Result<StartedSubAgents<S::StartedSubAgent>, SubAgentError> {
        let started_sub_agents: Result<HashMap<AgentID, S::StartedSubAgent>, SubAgentError> = self
            .0
            .into_iter()
            .map(|(id, subagent)| {
                let running_agent = subagent.run()?;
                Ok((id, running_agent))
            })
            .collect();
        Ok(StartedSubAgents(started_sub_agents?))
    }
}

pub(crate) struct StartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: StartedSubAgent;

impl<S> StartedSubAgents<S>
where
    S: StartedSubAgent,
{
    fn stop_agent(agent_id: &AgentID, sub_agent: S) -> Result<(), SubAgentError> {
        let result = sub_agent.stop()?;
        result.into_iter().for_each(|handle| {
            handle.join().map_or_else(
                |_err| {
                    // let error: &dyn std::error::Error = &err;
                    error!(
                        supervisor = agent_id.to_string(),
                        msg = "stopped with error",
                    )
                },
                |_| {
                    info!(
                        supervisor = agent_id.to_string(),
                        msg = "stopped successfully"
                    )
                },
            )
        });
        Ok(())
    }

    pub(crate) fn stop_remove(&mut self, agent_id: &AgentID) -> Result<(), SubAgentError> {
        // TODO: handle error
        let sub_agent = self.0.remove(agent_id).unwrap();
        Self::stop_agent(agent_id, sub_agent)?;
        Ok(())
    }

    pub(crate) fn insert(&mut self, agent_id: AgentID, sub_agent: S) {
        // TODO: handle error
        self.0.insert(agent_id, sub_agent).unwrap();
    }

    pub(crate) fn stop(self) -> Result<(), SubAgentError> {
        self.0.into_iter().try_for_each(|(agent_id, sub_agent)| {
            Self::stop_agent(&agent_id, sub_agent)?;
            Ok(())
        })
    }
}
