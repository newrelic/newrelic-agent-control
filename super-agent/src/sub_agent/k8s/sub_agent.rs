use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::k8s::CRSupervisor;
use crate::sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent};
use crate::sub_agent::{NotStarted, Started};
use crate::super_agent::config::AgentID;
use tracing::error;

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On K8s
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentK8s<S> {
    agent_id: AgentID,
    supervisor: Option<CRSupervisor>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    state: S,
}

impl<E> SubAgentK8s<NotStarted<E>>
where
    E: SubAgentEventProcessor,
{
    pub fn new(
        agent_id: AgentID,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        supervisor: Option<CRSupervisor>,
    ) -> Self {
        SubAgentK8s {
            agent_id,
            supervisor,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
        }
    }
}

impl<E> NotStartedSubAgent for SubAgentK8s<NotStarted<E>>
where
    E: SubAgentEventProcessor,
{
    type StartedSubAgent = SubAgentK8s<Started>;

    // Run has two main duties:
    // - it starts the supervisors if any
    // - it starts processing events (internal and opamp ones)
    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        if let Some(cr_supervisor) = &self.supervisor {
            cr_supervisor.apply().inspect_err(|err| {
                error!(
                    "The creation of the resources failed for '{}': '{}'",
                    self.agent_id, err
                )
            })?;
        }

        let event_loop_handle = self.state.event_processor.process();

        Ok(SubAgentK8s {
            agent_id: self.agent_id,
            supervisor: self.supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
        })
    }
}

impl StartedSubAgent for SubAgentK8s<Started> {
    // Stop does not delete directly the CR. It will be the garbage collector doing so if needed.
    fn stop(self) -> Result<Vec<std::thread::JoinHandle<()>>, SubAgentError> {
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;

        self.state.event_loop_handle.join().map_err(|_| {
            SubAgentError::PoisonError(String::from("error handling event_loop_handle"))
        })??;
        Ok(vec![])
    }
}
