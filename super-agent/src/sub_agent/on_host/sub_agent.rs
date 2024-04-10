use std::thread::JoinHandle;

use tracing::debug;

use super::supervisor::command_supervisor::SupervisorOnHost;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::error::SubAgentError;
use crate::super_agent::config::AgentID;

use super::supervisor::command_supervisor;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::{NotStarted, Started};
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On Host
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentOnHost<S, V> {
    supervisors: Vec<SupervisorOnHost<V>>,
    agent_id: AgentID,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    state: S,
}

impl<S, V> SubAgentOnHost<S, V> {
    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<E> SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted>
where
    E: SubAgentEventProcessor,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<SupervisorOnHost<command_supervisor::NotStarted>>,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted> {
        SubAgentOnHost {
            supervisors,
            agent_id,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
        }
    }
}

impl<E> NotStartedSubAgent for SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted>
where
    E: SubAgentEventProcessor,
{
    type StartedSubAgent = SubAgentOnHost<Started, command_supervisor::Started>;

    fn run(self) -> Result<SubAgentOnHost<Started, command_supervisor::Started>, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| {
                debug!("Running supervisor {} for {}", s.id(), self.agent_id);
                s.run(self.sub_agent_internal_publisher.clone())
            })
            .collect::<Result<Vec<_>, _>>()?;

        let event_loop_handles = self.state.event_processor.process()?;

        let started_sub_agent = SubAgentOnHost {
            supervisors: started_supervisors,
            agent_id: self.agent_id,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handles },
        };

        Ok(started_sub_agent)
    }
}

impl StartedSubAgent for SubAgentOnHost<Started, command_supervisor::Started> {
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;

        for handle in self.state.event_loop_handles {
            handle.join().map_err(|_| {
                SubAgentError::PoisonError(String::from("error handling event_loop_handle"))
            })?;
        }

        Ok(stopped_supervisors)
    }
}

#[cfg(test)]
mod test {
    use crate::event::channel::pub_sub;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::on_host::sub_agent::SubAgentOnHost;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentID;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_events_are_processed() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let supervisors = Vec::default();

        let mut event_processor = MockEventProcessorMock::default();
        event_processor.should_process();

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let sub_agent = SubAgentOnHost::new(
            agent_id,
            supervisors,
            event_processor,
            sub_agent_internal_publisher,
        );

        let started_agent = sub_agent.run().unwrap();
        sleep(Duration::from_millis(20));
        // close the OpAMP Publisher

        let handles = started_agent.stop().unwrap();

        for handle in handles {
            handle.join().unwrap();
        }
        println!("END")
    }
}
