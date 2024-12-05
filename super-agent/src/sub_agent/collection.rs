use super::{error::SubAgentCollectionError, NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::AgentID;
use std::collections::HashMap;
use tracing::{debug, info};

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
    pub(crate) fn run(self) -> StartedSubAgents<S::StartedSubAgent> {
        let sub_agents: HashMap<AgentID, S::StartedSubAgent> = self
            .0
            .into_iter()
            .map(|(id, subagent)| {
                debug!("Running supervisor for agent {}", id);
                (id, subagent.run())
            })
            .collect();
        StartedSubAgents(sub_agents)
    }
}

pub(crate) struct StartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: StartedSubAgent;

impl<S> StartedSubAgents<S>
where
    S: StartedSubAgent,
{
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

        info!(%agent_id, "Stopping sub agent");
        sub_agent.stop();

        Ok(())
    }

    pub(crate) fn insert(&mut self, agent_id: AgentID, sub_agent: S) -> Option<S> {
        // TODO: handle error
        self.0.insert(agent_id, sub_agent)
    }

    pub(crate) fn stop(self) {
        self.0.into_iter().for_each(|(agent_id, sub_agent)| {
            info!(%agent_id, "Stopping sub agent");
            sub_agent.stop();
        })
    }
}

#[cfg(test)]
pub mod tests {
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::StartedSubAgent;
    use crate::super_agent::config::AgentID;
    use std::collections::HashMap;

    impl<S> StartedSubAgents<S>
    where
        S: StartedSubAgent,
    {
        pub(crate) fn len(&self) -> usize {
            self.0.len()
        }
    }

    impl<S> From<HashMap<AgentID, S>> for StartedSubAgents<S>
    where
        S: StartedSubAgent,
    {
        fn from(value: HashMap<AgentID, S>) -> Self {
            StartedSubAgents(value)
        }
    }

    // allow creating an empty StartedSubAgents for testing
    impl<S> Default for StartedSubAgents<S>
    where
        S: StartedSubAgent,
    {
        fn default() -> Self {
            StartedSubAgents(HashMap::default())
        }
    }
}
