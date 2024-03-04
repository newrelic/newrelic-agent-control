use super::{
    error::{SubAgentCollectionError, SubAgentError},
    NotStartedSubAgent, StartedSubAgent,
};
use crate::super_agent::config::AgentID;
use std::collections::HashMap;
use tracing::{debug, error, info};

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
    pub(crate) fn run(
        self,
    ) -> Result<StartedSubAgents<S::StartedSubAgent>, SubAgentCollectionError> {
        let sub_agents: Result<HashMap<AgentID, S::StartedSubAgent>, SubAgentError> = self
            .0
            .into_iter()
            .map(|(id, subagent)| {
                debug!("Running supervisors for agent {}", id);
                Ok((id, subagent.run()?))
            })
            .collect();
        Ok(StartedSubAgents(sub_agents?))
    }
}

pub(crate) struct StartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: StartedSubAgent;

impl<S> StartedSubAgents<S>
where
    S: StartedSubAgent,
{
    fn stop_agent(agent_id: &AgentID, sub_agent: S) -> Result<(), SubAgentCollectionError> {
        let result = sub_agent.stop()?;
        result.into_iter().for_each(|handle| {
            handle.join().map_or_else(
                |_err| {
                    // let error: &dyn std::error::Error = &err;
                    error!("Supervisor for {} stopped with error", agent_id,)
                },
                |_| info!("Supervisor for {} stopped successfully", agent_id),
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

#[cfg(test)]
pub mod test {
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

        pub fn get(&mut self, agent_id: &AgentID) -> &mut S {
            self.0.get_mut(agent_id).unwrap()
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
