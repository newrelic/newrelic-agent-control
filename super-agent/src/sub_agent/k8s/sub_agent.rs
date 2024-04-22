use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::sub_agent::health::health_checker::publish_health_event;
use crate::sub_agent::health::health_checker::Unhealthy;
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
    ch_stop_health: Option<EventPublisher<()>>,
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
            ch_stop_health: None,
            state: NotStarted { event_processor },
        }
    }

    fn handle_error(&self, err: &SupervisorError, msg: &str) {
        let last_error = format!("{msg}: {err}");

        let event = SubAgentInternalEvent::AgentBecameUnhealthy(Unhealthy {
            last_error,
            ..Default::default()
        });

        error!(%self.agent_id,%err,msg);
        publish_health_event(&self.sub_agent_internal_publisher, event);
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
        let mut ch_stop_health = None;

        if let Some(cr_supervisor) = &self.supervisor {
            ch_stop_health = cr_supervisor
                .apply()
                .inspect_err(|err| {
                    self.handle_error(err, "the creation of the resources failed");
                })
                .and_then(|resources| {
                    cr_supervisor
                        .start_monitor_health(self.sub_agent_internal_publisher.clone(), resources)
                })
                .inspect_err(|err| {
                    self.handle_error(err, "starting monitoring resources failed");
                })
                .ok();
        }

        let event_loop_handle = self.state.event_processor.process();

        SubAgentK8s {
            agent_id: self.agent_id,
            agent_type: self.agent_type,
            supervisor: self.supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            ch_stop_health,
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
        // Stop processing events
        self.sub_agent_internal_publisher
            .publish(SubAgentInternalEvent::StopRequested)?;

        // Stopping health checkers if configured
        if let Some(csh) = self.ch_stop_health {
            csh.publish(())?;
        }

        self.state.event_loop_handle.join().map_err(|_| {
            SubAgentError::PoisonError(String::from("error handling event_loop_handle"))
        })??;
        Ok(vec![])
    }
}

#[cfg(test)]
pub mod test {
    use crate::event::channel::{pub_sub, EventPublisher};
    use crate::event::SubAgentInternalEvent;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::k8s::builder::test::k8s_sample_runtime_config;
    use crate::sub_agent::k8s::sub_agent::SubAgentK8s;
    use crate::sub_agent::k8s::{CRSupervisor, SupervisorError};
    use crate::sub_agent::{NotStarted, NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{helm_release_type_meta, AgentID, AgentTypeFQN};
    use assert_matches::assert_matches;
    use kube::api::DynamicObject;
    use std::sync::Arc;
    use std::time::Duration;

    const TEST_K8S_ISSUE: &str = "random issue";
    pub const TEST_AGENT_ID: &str = "k8s-test";
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

        match sub_agent_internal_consumer
            .as_ref()
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
        {
            SubAgentInternalEvent::AgentBecameUnhealthy(unhealthy) => {
                assert!(unhealthy.last_error().contains(TEST_K8S_ISSUE))
            }
            _ => panic!("AgentBecameUnhealthy event expected"),
        }
    }

    #[test]
    fn build_start_monitor_fails() {
        // Monitoring fail since the resource has no valid metadata
        let (sub_agent_internal_publisher, _) = pub_sub();

        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let k8s_obj = k8s_sample_runtime_config(true);
        let mock_client = MockSyncK8sClient::default();

        let supervisor_res = CRSupervisor::new(agent_id.clone(), Arc::new(mock_client), k8s_obj)
            .start_monitor_health(
                sub_agent_internal_publisher,
                vec![DynamicObject {
                    types: Some(helm_release_type_meta()),
                    metadata: Default::default(),
                    data: Default::default(),
                }],
            );

        assert_matches!(
            supervisor_res.err().unwrap(),
            SupervisorError::HealthError(_)
        );
    }

    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();

        let mut k8s_obj = k8s_sample_runtime_config(true);
        // This corresponds to 0.5 seconds
        k8s_obj.health.interval = Duration::new(0, 1);

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

        let supervisor = CRSupervisor::new(agent_id.clone(), Arc::new(mock_client), k8s_obj);

        SubAgentK8s::new(
            agent_id.clone(),
            AgentTypeFQN::from("test:0.0.1"),
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
                panic!("AgentBecameHealthy event expected")
            }
        }
    }

    fn create_k8s_sub_agent_successfully(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        k8s_client_should_fail: bool,
    ) -> SubAgentK8s<NotStarted<MockEventProcessorMock>> {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();

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

        let supervisor = CRSupervisor::new(
            agent_id.clone(),
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
