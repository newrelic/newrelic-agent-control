use super::{StartedSubAgent, error::SubAgentCollectionError};
use crate::agent_control::agent_id::AgentID;
use std::collections::HashMap;
use tracing::{error, info};

pub(crate) struct StartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: StartedSubAgent;

impl<S> StartedSubAgents<S>
where
    S: StartedSubAgent,
{
    #[tracing::instrument(skip_all)]
    pub(crate) fn stop_and_remove(
        &mut self,
        agent_id: &AgentID,
    ) -> Result<(), SubAgentCollectionError> {
        let sub_agent =
            self.0
                .remove(agent_id)
                .ok_or(SubAgentCollectionError::SubAgentNotFound(
                    agent_id.to_string(),
                ))?;

        info!("Stopping sub agent");
        Self::stop_sub_agent(sub_agent);

        Ok(())
    }

    pub(crate) fn insert(&mut self, agent_id: AgentID, sub_agent: S) -> Option<S> {
        // TODO: handle error
        self.0.insert(agent_id, sub_agent)
    }

    pub(crate) fn stop(self) {
        self.0.into_iter().for_each(|(_, sub_agent)| {
            info!("Stopping sub agent");
            Self::stop_sub_agent(sub_agent);
        })
    }

    #[tracing::instrument(skip_all)]
    fn stop_sub_agent(sub_agent: S) {
        let _ = sub_agent
            .stop()
            .inspect_err(|err| error!(%err, "Error stopping sub agent"));
    }
}

impl<S> Default for StartedSubAgents<S>
where
    S: StartedSubAgent,
{
    fn default() -> Self {
        StartedSubAgents(HashMap::default())
    }
}

#[cfg(test)]
pub mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::sub_agent::StartedSubAgent;
    use crate::sub_agent::collection::StartedSubAgents;
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
}
