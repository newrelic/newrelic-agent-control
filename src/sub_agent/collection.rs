use std::collections::HashMap;

use crate::config::super_agent_configs::AgentID;

use tracing::{error, info};

use super::{
    error::{SubAgentCollectionError, SubAgentError},
    SubAgent,
};

pub(crate) struct SubAgents<S>(HashMap<AgentID, S>)
where
    S: SubAgent;

impl<S> From<HashMap<AgentID, S>> for SubAgents<S>
where
    S: SubAgent,
{
    fn from(value: HashMap<AgentID, S>) -> Self {
        Self(value)
    }
}

impl<S> SubAgents<S>
where
    S: SubAgent,
{
    pub(crate) fn run(
        self,
    ) -> Result<SubAgents<S>, SubAgentCollectionError> {
        let sub_agents: Result<HashMap<AgentID, S>, SubAgentError> = self
            .0
            .into_iter()
            .map(|(id, mut subagent)| {
                subagent.run()?;
                Ok((id, subagent))
            })
            .collect();
        Ok(SubAgents(sub_agents?))
    }

    fn stop_agent(agent_id: &AgentID, sub_agent: S) -> Result<(), SubAgentCollectionError> {
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

    pub(crate) fn stop_remove(
        &mut self,
        agent_id: &AgentID,
    ) -> Result<(), SubAgentCollectionError> {
        let sub_agent =
            self.0
                .remove(agent_id)
                .ok_or(SubAgentCollectionError::SubAgentNotFound(
                    agent_id.to_string(),
                ))?;
        Self::stop_agent(agent_id, sub_agent)
    }

    pub(crate) fn insert(&mut self, agent_id: AgentID, sub_agent: S) -> Option<S> {
        // TODO: handle error
        self.0.insert(agent_id, sub_agent)
    }

    pub(crate) fn stop(self) -> Result<(), SubAgentCollectionError> {
        self.0.into_iter().try_for_each(|(agent_id, sub_agent)| {
            Self::stop_agent(&agent_id, sub_agent)?;
            Ok(())
        })
    }
}
