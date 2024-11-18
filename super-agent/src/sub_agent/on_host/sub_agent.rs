#[cfg(test)]
pub(crate) mod test {
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::event::OpAMPEvent;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::config_validator::ConfigValidator;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::on_host::supervisor::command_supervisor;
    use crate::sub_agent::on_host::supervisor::command_supervisor::SupervisorOnHost;
    use crate::sub_agent::supervisor::SupervisorBuilder;
    use crate::sub_agent::SubAgent;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig};
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use mockall::{mock, predicate};
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Applying;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    // Mock for the OnHost supervisor builder (the associated type needs to be set, therefore we cannot define a generic mock).
    mock! {
        pub SupervisorBuilderOnhost {}

        impl SupervisorBuilder for SupervisorBuilderOnhost {
            type SupervisorStarter = SupervisorOnHost<command_supervisor::NotStarted>;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
                maybe_opamp_client: &Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>>,
            ) -> Result<Option<SupervisorOnHost<command_supervisor::NotStarted>>, SubAgentBuilderError>;
        }
    }

    #[test]
    fn test_run_and_stop() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        let sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = on_host_final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
            1,
        );

        let mut supervisor_builder = MockSupervisorBuilderOnhost::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(
                predicate::function(move |e: &Result<EffectiveAgent, _>| {
                    e.as_ref().is_ok_and(|x| *x == effective_agent)
                }),
                predicate::always(),
            )
            .returning(|_, _| Ok(None));

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            none_mock_opamp_client(),
            supervisor_builder,
            sub_agent_publisher,
            None,
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        //start the runtime
        let started_agent = sub_agent.run();

        sleep(Duration::from_millis(20));
        // stop the runtime
        started_agent.stop();
    }

    #[traced_test]
    #[test]
    fn test_remote_config() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        let mut sub_agent_remote_config_hash_repository = MockHashRepositoryMock::default();
        let mut remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let effective_agent = on_host_final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let mut assembler = MockEffectiveAgentAssemblerMock::new();
        assembler.should_assemble_agent(
            &agent_id,
            &agent_cfg,
            &Environment::OnHost,
            effective_agent.clone(),
            2,
        );

        let mut supervisor_builder = MockSupervisorBuilderOnhost::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(
                predicate::function(move |e: &Result<EffectiveAgent, _>| {
                    e.as_ref().is_ok_and(|x| *x == effective_agent)
                }),
                predicate::always(),
            )
            .returning(|_, _| Ok(None));

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        sub_agent_remote_config_hash_repository.should_save_hash(&agent_id, &hash);
        remote_values_repo.should_store_remote(
            &agent_id,
            &YAMLConfig::new(HashMap::from([("some_item".into(), "some_value".into())])),
        );

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();

        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        });

        opamp_client
            .expect_update_effective_config()
            .once()
            .returning(|| Ok(()));

        //opamp client expects to be stopped
        opamp_client.should_stop(1);

        let sub_agent = SubAgent::new(
            agent_id,
            agent_cfg,
            Arc::new(assembler),
            Some(opamp_client),
            supervisor_builder,
            sub_agent_publisher,
            Some(sub_agent_opamp_consumer),
            (sub_agent_internal_publisher, sub_agent_internal_consumer),
            Arc::new(sub_agent_remote_config_hash_repository),
            Arc::new(remote_values_repo),
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            Environment::OnHost,
        );

        //start the runtime
        let started_agent = sub_agent.run();

        // publish event
        sub_agent_opamp_publisher
            .publish(OpAMPEvent::RemoteConfigReceived(remote_config))
            .unwrap();
        sleep(Duration::from_millis(20));

        // stop the runtime
        started_agent.stop();

        assert!(logs_with_scope_contain(
            "DEBUG newrelic_super_agent::sub_agent::sub_agent",
            "remote config received",
        ));
    }

    pub(crate) fn on_host_final_agent(
        agent_id: AgentID,
        agent_fqn: AgentTypeFQN,
    ) -> EffectiveAgent {
        use crate::agent_type::definition::TemplateableValue;

        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost {
                        executable: None,
                        enable_file_logging: TemplateableValue::new(false),
                        health: None,
                    }),
                    k8s: None,
                },
            },
        )
    }

    fn none_mock_opamp_client(
    ) -> Option<MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>> {
        None
    }
}
