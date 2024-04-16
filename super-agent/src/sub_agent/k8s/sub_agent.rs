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
    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        if let Some(cr_supervisor) = &self.supervisor {
            _ = cr_supervisor
                .apply()
                .inspect_err(|err| self.handle_supervisor_error(err));
        }

        let event_loop_handle = self.state.event_processor.process();

        Ok(SubAgentK8s {
            agent_id: self.agent_id,
            agent_type: self.agent_type,
            supervisor: self.supervisor,
            sub_agent_internal_publisher: self.sub_agent_internal_publisher,
            state: Started { event_loop_handle },
        })
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
    use crate::agent_type::agent_metadata::AgentMetadata;
    use crate::agent_type::environment::Environment;
    use crate::event::channel::pub_sub;
    use crate::event::SubAgentInternalEvent;
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::opamp::client_builder::test::{
        MockOpAMPClientBuilderMock, MockStartedOpAMPClientMock,
    };
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::k8s::builder::test::k8s_effective_agent;
    use crate::sub_agent::k8s::builder::K8sSubAgentBuilder;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentBuilder};
    use crate::super_agent::config::{AgentID, K8sConfig, SubAgentConfig};
    use opamp_client::operation::settings::DescriptionValueType;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let cluster_name = "test-cluster";
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let effective_agent = k8s_effective_agent(AgentID::new("k8s-test").unwrap(), true);
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentMetadata::default().to_string().as_str().into(),
        };
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(cluster_name.to_string()),
            )]),
        );

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);

        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            started_client,
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Ok(()));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let sub_agent_id = AgentID::new("k8s-test").unwrap();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::K8s,
            effective_agent,
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        _ = sub_agent_event_processor_builder.should_return_event_processor_with_consumer();

        let k8s_config = K8sConfig {
            cluster_name: cluster_name.to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (application_event_publisher, _application_event_consumer) = pub_sub();
        let started_agent = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                application_event_publisher,
            )
            .unwrap() // Not started agent
            .run()
            .unwrap();
        assert!(started_agent.stop().is_ok())
    }

    #[test]
    fn build_start_fails() {
        let test_issue = "random issue";
        let cluster_name = "test-cluster";

        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let effective_agent = crate::sub_agent::k8s::builder::test::k8s_effective_agent(
            AgentID::new("k8s-test").unwrap(),
            true,
        );
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentMetadata::default().to_string().as_str().into(),
        };
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(cluster_name.to_string()),
            )]),
        );

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);

        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            started_client,
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        // instance K8s client mock now FAILING to apply
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Err(K8sError::GetDynamic(test_issue.to_string())));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let sub_agent_id = AgentID::new("k8s-test").unwrap();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::K8s,
            effective_agent,
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        // Setting a random variant of the enum to make the compiler happy
        let test_consumer =
            sub_agent_event_processor_builder.should_return_event_processor_with_consumer();

        let k8s_config = K8sConfig {
            cluster_name: "test-cluster".to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (application_event_publisher, _application_event_consumer) = pub_sub();

        _ = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                application_event_publisher,
            )
            .unwrap() // Not started agent
            .run();

        match test_consumer.as_ref().recv().unwrap() {
            SubAgentInternalEvent::AgentBecameUnhealthy(message) => {
                assert!(message.contains(test_issue))
            }
            _ => panic!("AgentBecameUnhealthy event expected"),
        }
    }
}
