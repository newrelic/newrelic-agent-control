//! A keyed collection of started sub-agents, supporting bulk and individual stopping.

use super::{StartedSubAgent, SubAgentJoinHandle, error::SubAgentCollectionError};
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
        self.0.insert(agent_id, sub_agent)
    }

    pub(crate) fn stop(self) {
        let handles: Vec<_> = self
            .0
            .into_iter()
            .filter_map(|(agent_id, sub_agent)| {
                info!(%agent_id, "Stopping sub agent");
                sub_agent
                    .stop()
                    .inspect_err(|err| error!(%agent_id, "Error stopping sub agent: {err}"))
                    .ok()
                    .map(|handle| (agent_id, handle))
            })
            .collect();

        for (agent_id, handle) in handles {
            let _ = handle
                .join()
                .inspect_err(|err| error!(%agent_id, "Error joining sub agent thread: {err}"));
            info!(%agent_id, "Sub agent stopped");
        }
    }

    #[tracing::instrument(skip_all)]
    fn stop_sub_agent(sub_agent: S) {
        let _ = sub_agent
            .stop()
            .and_then(SubAgentJoinHandle::join)
            .inspect_err(|err| {
                error!("Error stopping sub agent: {err}");
            });
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
#[allow(missing_docs)]
pub mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::tests::MockStartedSubAgent;
    use crate::sub_agent::{StartedSubAgent, SubAgentJoinHandle};
    use std::collections::HashMap;
    use std::thread::{sleep, spawn};
    use std::time::{Duration, Instant};

    fn sub_agent_stopping_in(stop_time: Duration) -> MockStartedSubAgent {
        let mut sub_agent = MockStartedSubAgent::new();
        sub_agent.expect_stop().once().return_once(move || {
            Ok(SubAgentJoinHandle(spawn(move || {
                sleep(stop_time);
                Ok(())
            })))
        });
        sub_agent
    }

    #[test]
    fn stop_takes_as_long_as_the_slowest_sub_agent() {
        let fast = Duration::from_millis(300);
        let slow = Duration::from_millis(500);
        let tolerance = Duration::from_millis(100);

        let mut sub_agents = StartedSubAgents::default();
        for i in 0..20 {
            let id = format!("fast-{i}").try_into().unwrap();
            sub_agents.insert(id, sub_agent_stopping_in(fast));
        }
        sub_agents.insert("slow".try_into().unwrap(), sub_agent_stopping_in(slow));

        let start = Instant::now();
        sub_agents.stop();
        let elapsed = start.elapsed();

        // Stopping sequentially would take 20 * fast + slow (2.5s).
        assert!(
            elapsed < slow + tolerance,
            "sub-agents were not stopped concurrently, stop took {elapsed:?}"
        );
    }

    impl<S> StartedSubAgents<S>
    where
        S: StartedSubAgent,
    {
        pub(crate) fn agents(&mut self) -> &mut HashMap<AgentID, S> {
            &mut self.0
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

    impl<S> From<StartedSubAgents<S>> for HashMap<AgentID, S>
    where
        S: StartedSubAgent,
    {
        fn from(value: StartedSubAgents<S>) -> Self {
            value.0
        }
    }
}
