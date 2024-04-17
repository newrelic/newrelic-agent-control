use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::k8s::{CRSupervisor, SupervisorError};
use crate::sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent};
use crate::sub_agent::{NotStarted, Started};
use crate::super_agent::config::{AgentID, AgentTypeFQN};
use tracing::error;

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On K8s
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentK8s<S> {
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
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
        agent_type: AgentTypeFQN,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        supervisor: Option<CRSupervisor>,
    ) -> Self {
        SubAgentK8s {
            agent_id,
            agent_type,
            supervisor,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
        }
    }

    fn handle_supervisor_error(&self, err: &SupervisorError) {
        let msg = "the creation of the resources failed";
        let event = SubAgentInternalEvent::AgentBecameUnhealthy(format!("{msg}: {err}"));

        error!(
            agent_id = self.agent_id.to_string(),
            err = err.to_string(),
            msg,
        );
        let _ = self
            .sub_agent_internal_publisher
            .publish(event)
            .inspect_err(|event_err| {
                error!(
                    agent_id = self.agent_id.to_string(),
                    err = event_err.to_string(),
                    "cannot publish unhealthy event"
                );
            });
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
    fn run(self) -> Self::StartedSubAgent {
        if let Some(cr_supervisor) = &self.supervisor {
            _ = cr_supervisor
                .apply()
                .inspect_err(|err| self.handle_supervisor_error(err));
        }

        let event_loop_handle = self.state.event_processor.process();

        SubAgentK8s {
            agent_id: self.agent_id,
            agent_type: self.agent_type,
            supervisor: self.supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
        }
    }
}

impl StartedSubAgent for SubAgentK8s<Started> {
    fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }

    fn agent_type(&self) -> AgentTypeFQN {
        self.agent_type.clone()
    }

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

#[cfg(test)]
mod test {
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::SubAgentInternalEvent;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::k8s::builder::test::k8s_sample_obj;
    use crate::sub_agent::k8s::sub_agent::SubAgentK8s;
    use crate::sub_agent::k8s::CRSupervisor;
    use crate::sub_agent::{NotStarted, NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use assert_matches::assert_matches;
    use std::sync::Arc;

    const TEST_K8S_ISSUE: &str = "random issue";

    #[test]
    fn k8s_sub_agent_start_and_stop() {
        let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();

        let started_agent =
            create_k8s_sub_agent_successfully(sub_agent_internal_publisher, false).run();
        assert!(started_agent.stop().is_ok());
    }

    #[test]
    fn k8s_sub_agent_start_and_fail_stop() {
        let (sub_agent_internal_publisher, _) = pub_sub();

        let started_agent =
            create_k8s_sub_agent_successfully(sub_agent_internal_publisher, false).run();

        // This error is triggered since the consumer is dropped and therefore the channel is closed
        // Therefore, the subAgent fails to write to such channel when stopping
        assert_matches!(
            started_agent.stop().unwrap_err(),
            SubAgentError::EventPublisherError(_)
        );
    }

    #[test]
    fn build_start_fails() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let started_agent =
            create_k8s_sub_agent_successfully(sub_agent_internal_publisher, true).run();
        assert!(started_agent.stop().is_ok());

        match sub_agent_internal_consumer.as_ref().recv().unwrap() {
            SubAgentInternalEvent::AgentBecameUnhealthy(message) => {
                assert!(message.contains(TEST_K8S_ISSUE))
            }
            _ => panic!("AgentBecameUnhealthy event expected"),
        }
    }

    fn create_k8s_sub_agent_successfully(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        k8s_client_should_fail: bool,
    ) -> SubAgentK8s<NotStarted<MockEventProcessorMock>> {
        let agent_id = AgentID::new("k8s-test").unwrap();

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(move |_| match k8s_client_should_fail {
                true => Err(K8sError::GetDynamic(TEST_K8S_ISSUE.to_string())),
                false => Ok(()),
            });
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let mut processor = MockEventProcessorMock::new();
        processor.should_process();

        let k8s_obj = k8s_sample_obj(true);
        let supervisor = CRSupervisor::new(agent_id.clone(), Arc::new(mock_client), k8s_obj);

        SubAgentK8s::new(
            agent_id.clone(),
            AgentTypeFQN::from("test:0.0.1"),
            processor,
            sub_agent_internal_publisher.clone(),
            Some(supervisor),
        )
    }
}
