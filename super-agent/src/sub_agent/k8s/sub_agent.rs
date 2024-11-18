#[cfg(test)]
pub mod test {
    use crate::agent_type::environment::Environment;
    use crate::agent_type::health_config::K8sHealthConfig;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::channel::{pub_sub, EventConsumer, EventPublisher};
    use crate::event::{SubAgentEvent, SubAgentInternalEvent};
    use crate::k8s::client::MockSyncK8sClient;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::sub_agent::config_validator::ConfigValidator;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::k8s::builder::test::k8s_sample_runtime_config;
    use crate::sub_agent::k8s::supervisor::NotStartedSupervisorK8s;
    use crate::sub_agent::supervisor::SupervisorBuilder;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgent};
    use crate::super_agent::config::{
        helm_release_type_meta, AgentID, AgentTypeFQN, SubAgentConfig,
    };
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use kube::api::DynamicObject;
    use mockall::{mock, predicate};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::traced_test;

    pub const TEST_AGENT_ID: &str = "k8s-test";
    pub const TEST_GENT_FQN: &str = "ns/test:0.1.2";

    // Mock for the k8s supervisor builder (the associated type needs to be set, therefore we cannot define a generic mock).
    mock! {
        pub SupervisorBuilderK8s {}

        impl SupervisorBuilder for SupervisorBuilderK8s {
            type SupervisorStarter = NotStartedSupervisorK8s;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
                maybe_opamp_client: &Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>>,
            ) -> Result<Option<NotStartedSupervisorK8s>, SubAgentBuilderError>;
        }
    }

    // Testing type to make usage more readable
    type SubAgentK8sForTesting = SubAgent<
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>,
        AgentCallbacks<MockEffectiveConfigLoaderMock>,
        MockEffectiveAgentAssemblerMock,
        MockSupervisorBuilderK8s,
        MockHashRepositoryMock,
        MockYAMLConfigRepositoryMock,
    >;

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_stop() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let started_agent = testing_k8s_sub_agent(
            sub_agent_internal_publisher,
            sub_agent_internal_consumer,
            sub_agent_publisher,
            || None,
        )
        .run();

        started_agent.stop();

        assert!(!logs_contain("ERROR"));
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_fail_stop() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let started_agent = testing_k8s_sub_agent(
            sub_agent_internal_publisher.clone(),
            sub_agent_internal_consumer,
            sub_agent_publisher,
            || None,
        )
        .run();

        // We send a message to end the runtime
        let publishing_result =
            sub_agent_internal_publisher.publish(SubAgentInternalEvent::StopRequested);
        assert!(publishing_result.is_ok());

        // And wait enough time for the runtime to consume the event so the loop broken
        sleep(Duration::from_millis(20));

        started_agent.stop();
        // This error is triggered since we try to send a StopRequested event on a closed channel
        assert!(logs_contain("Error stopping event loop"));
    }

    #[traced_test]
    #[test]
    fn k8s_sub_agent_start_and_monitor_health() {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();

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
        let mock_k8s_client = Arc::new(mock_client);

        let agent_id_for_builder = agent_id.clone();

        let supervisor_fn = move || {
            Some(NotStartedSupervisorK8s::new(
                agent_id_for_builder.clone(),
                agent_fqn.clone(),
                mock_k8s_client.clone(),
                k8s_obj.clone(),
            ))
        };

        // If the started subagent is dropped, then the underlying supervisor is also dropped (and the underlying tasks are stopped)
        let _ = testing_k8s_sub_agent(
            sub_agent_internal_publisher,
            sub_agent_internal_consumer,
            sub_agent_publisher,
            supervisor_fn,
        )
        .run();

        let timeout = Duration::from_secs(3);

        match sub_agent_consumer.as_ref().recv_timeout(timeout).unwrap() {
            SubAgentEvent::SubAgentHealthInfo(_, _, h) => {
                if h.is_healthy() {
                    panic!("unhealthy event expected")
                }
            }
        }
    }

    /// Sets up a k8s sub agent for testing and the corresponding underlying mocks. The supervisor builder will
    /// call the provided `supervisor_fn` to return the corresponding supervisor.
    fn testing_k8s_sub_agent<F: Fn() -> Option<NotStartedSupervisorK8s> + Send + 'static>(
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        supervisor_fn: F,
    ) -> SubAgentK8sForTesting {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let agent_fqn = AgentTypeFQN::try_from(TEST_GENT_FQN).unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: agent_fqn.clone(),
        };
        let k8s_config = k8s_sample_runtime_config(true);
        let runtime_config = Runtime {
            deployment: Deployment {
                k8s: Some(k8s_config),
                ..Default::default()
            },
        };
        let effective_agent =
            EffectiveAgent::new(agent_id.clone(), agent_fqn.clone(), runtime_config.clone());

        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::K8s,
            effective_agent,
            1,
        );

        let mut supervisor_builder = MockSupervisorBuilderK8s::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::always(), predicate::always())
            .returning(move |_, _| Ok(supervisor_fn()));

        let sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        SubAgent::new(
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(assembler),
            none_mock_opamp_client(),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (
                sub_agent_internal_publisher.clone(),
                sub_agent_internal_consumer,
            ),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::K8s,
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
