use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::k8s::NotStartedSupervisor;
use crate::sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent};
use crate::sub_agent::{NotStarted, Started};
use crate::super_agent::config::{AgentID, AgentTypeFQN};

use super::supervisor::log_and_report_unhealthy;
use super::supervisor::StartedSupervisor;

////////////////////////////////////////////////////////////////////////////////////
// SubAgent On K8s
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentK8s<S, V> {
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    supervisor: Option<V>,
    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    state: S,
}

impl<E> SubAgentK8s<NotStarted<E>, NotStartedSupervisor>
where
    E: SubAgentEventProcessor,
{
    pub fn new(
        agent_id: AgentID,
        agent_type: AgentTypeFQN,
        event_processor: E,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        supervisor: Option<NotStartedSupervisor>,
    ) -> Self {
        SubAgentK8s {
            agent_id,
            agent_type,
            supervisor,
            sub_agent_internal_publisher,
            state: NotStarted { event_processor },
        }
    }
}

impl<E> NotStartedSubAgent for SubAgentK8s<NotStarted<E>, NotStartedSupervisor>
where
    E: SubAgentEventProcessor,
{
    type StartedSubAgent = SubAgentK8s<Started, StartedSupervisor>;

    // Run has two main duties:
    // - it starts the supervisors if any
    // - it starts processing events (internal and opamp ones)
    fn run(self) -> Self::StartedSubAgent {
        let maybe_started_supervisor = self
            .supervisor
            .map(|s| s.start(self.sub_agent_internal_publisher.clone()))
            .transpose()
            .inspect_err(|err| {
                log_and_report_unhealthy(
                    &self.sub_agent_internal_publisher,
                    err,
                    "starting the k8s resources supervisor failed",
                )
            })
            .unwrap_or(None);

        let event_loop_handle = self.state.event_processor.process();

        SubAgentK8s {
            agent_id: self.agent_id,
            agent_type: self.agent_type,
            supervisor: maybe_started_supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
        }
    }
}

impl StartedSubAgent for SubAgentK8s<Started, StartedSupervisor> {
    fn agent_id(&self) -> AgentID {
        self.agent_id.clone()
    }

    fn agent_type(&self) -> AgentTypeFQN {
        self.agent_type.clone()
    }

    // Stop does not delete directly the CR. It will be the garbage collector doing so if needed.
    fn stop(self) -> Result<Vec<std::thread::JoinHandle<()>>, SubAgentError> {
        // stop the k8s object supervisor
        let join_handles = self
            .supervisor
            .map(|s| s.stop())
            .transpose()?
            .unwrap_or_default();
        // Stop processing events
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;

        self.state.event_loop_handle.join().map_err(|_| {
            SubAgentError::PoisonError(String::from("error handling event_loop_handle"))
        })??;
        Ok(join_handles)
    }
}

#[cfg(test)]
pub mod test {
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::SubAgentInternalEvent;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::k8s::builder::test::k8s_sample_runtime_config;
    use crate::sub_agent::k8s::sub_agent::SubAgentK8s;
    use crate::sub_agent::k8s::NotStartedSupervisor;
    use crate::sub_agent::{NotStarted, NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{helm_release_type_meta, AgentID, AgentTypeFQN};
    use assert_matches::assert_matches;
    use kube::api::DynamicObject;
    use std::sync::Arc;
    use std::time::Duration;

    const TEST_K8S_ISSUE: &str = "random issue";
    pub const TEST_AGENT_ID: &str = "k8s-test";
    pub const TEST_GENT_FQN: &str = "ns/test:0.1.2";
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
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

        let mut k8s_obj = k8s_sample_runtime_config(true);
        k8s_obj.health = Some(K8sHealthConfig {
            interval: Duration::from_millis(500).into(),
        });

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .returning(|_| Ok(()));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());
        mock_client.expect_get_helm_release().returning(|_| {
            Ok(Some(Arc::new(DynamicObject {
                types: Some(helm_release_type_meta()),
                metadata: Default::default(),
                data: Default::default(),
            })))
        });

        let mut processor = MockEventProcessorMock::new();
        processor.should_process();

        let supervisor =
            NotStartedSupervisor::new(agent_id.clone(), agent_fqn, Arc::new(mock_client), k8s_obj);

        // If the started subagent is dropped, then the underlying supervisor is also dropped (and the underlying tasks are stopped)
        let _started_subagent = SubAgentK8s::new(
            agent_id.clone(),
            AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap(),
            processor,
            sub_agent_internal_publisher.clone(),
            Some(supervisor),
        )
        .run();

        let timeout = Duration::from_secs(3);

        match sub_agent_internal_consumer
            .as_ref()
            .recv_timeout(timeout)
            .unwrap()
        {
            SubAgentInternalEvent::AgentBecameUnhealthy(_) => {}
            _ => {
                panic!("AgentBecameUnhealthy event expected")
            }
        }
    }

    fn create_k8s_sub_agent_successfully(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        k8s_client_should_fail: bool,
    ) -> SubAgentK8s<NotStarted<MockEventProcessorMock>, NotStartedSupervisor> {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();

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

        let supervisor = NotStartedSupervisor::new(
            agent_id.clone(),
            agent_fqn,
            Arc::new(mock_client),
            k8s_sample_runtime_config(true),
        );

        SubAgentK8s::new(
            agent_id.clone(),
            AgentTypeFQN::try_from("namespace/test:0.0.1").unwrap(),
            processor,
            sub_agent_internal_publisher.clone(),
            Some(supervisor),
        )
    }
}
