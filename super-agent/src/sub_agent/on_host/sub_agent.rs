use super::health_checker::{HealthChecker, HealthCheckerNotStarted, HealthCheckerStarted};
use super::supervisor::command_supervisor;
use super::supervisor::command_supervisor::SupervisorOnHost;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::{NotStarted, Started};
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::config::{AgentID, AgentTypeFQN};
use tracing::{debug, error};

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On Host
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentOnHost<S, V, H> {
    supervisor: Option<SupervisorOnHost<V>>,
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    // would make sense to move it to state and share implementation with k8s?
    health_checker: Option<HealthChecker<H>>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    state: S,
}

impl<E> SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted, HealthCheckerNotStarted>
where
    E: SubAgentEventProcessor,
{
    pub fn new(
        agent_id: AgentID,
        agent_type: AgentTypeFQN,
        health: Option<HealthChecker<HealthCheckerNotStarted>>,
        supervisor: Option<SupervisorOnHost<command_supervisor::NotStarted>>,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted, HealthCheckerNotStarted>
    {
        SubAgentOnHost {
            supervisor,
            agent_id,
            agent_type,
            health_checker: health,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
        }
    }
}

impl<E> NotStartedSubAgent
    for SubAgentOnHost<NotStarted<E>, command_supervisor::NotStarted, HealthCheckerNotStarted>
where
    E: SubAgentEventProcessor,
{
    type StartedSubAgent =
        SubAgentOnHost<Started, command_supervisor::Started, HealthCheckerStarted>;

    fn run(self) -> SubAgentOnHost<Started, command_supervisor::Started, HealthCheckerStarted> {
        let started_supervisor = self.supervisor.map(|s| {
            debug!("Running supervisor {} for {}", s.id(), self.agent_id);
            s.run(self.sub_agent_internal_publisher.clone())
        });

        let event_loop_handle = self.state.event_processor.process();

        let started_health_checker = self.health_checker.map(|h| h.start());

        SubAgentOnHost {
            supervisor: started_supervisor,
            agent_id: self.agent_id,
            agent_type: self.agent_type,
            health_checker: started_health_checker,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
        }
    }
}

impl StartedSubAgent
    for SubAgentOnHost<Started, command_supervisor::Started, HealthCheckerStarted>
{
    fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }

    fn agent_type(&self) -> AgentTypeFQN {
        self.agent_type.clone()
    }

    fn stop(self) {
        if let Some(h) = self.health_checker {
            h.stop();
        }

        if let Some(s) = self.supervisor {
            let _ = s.stop().join().inspect_err(|_| {
                error!(
                    agent_id = %self.agent_id,
                    "Error stopping supervisor thread"
                );
            });
        };

        let _ = self
            .sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)
            .inspect_err(|err| {
                error!(
                    agent_id = %self.agent_id,
                    %err,
                    "Error stopping event loop"
                )
            })
            .inspect(|_| {
                let _ = self.state.event_loop_handle.join().inspect_err(|_| {
                    error!(
                        agent_id = %self.agent_id,
                        "Error stopping event thread"
                    );
                });
            });
    }
}

#[cfg(test)]
mod test {
    use crate::event::channel::pub_sub;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::on_host::sub_agent::SubAgentOnHost;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_events_are_processed() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type = AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap();

        let mut event_processor = MockEventProcessorMock::default();
        event_processor.should_process();

        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();
        let sub_agent = SubAgentOnHost::new(
            agent_id,
            agent_type,
            None,
            None,
            event_processor,
            sub_agent_internal_publisher,
        );

        let started_agent = sub_agent.run();
        sleep(Duration::from_millis(20));
        // close the OpAMP Publisher

        started_agent.stop();

        println!("END")
    }
}
