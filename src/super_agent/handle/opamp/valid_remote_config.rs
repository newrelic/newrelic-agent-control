use std::sync::mpsc::Sender;

use opamp_client::StartedClient;
use tracing::{error, info};

use crate::{
    config::{agent_values::AgentValues, store::SubAgentsConfigStore},
    event::{channel::EventPublisher, OpAMPEvent},
    opamp::{
        remote_config::RemoteConfig,
        remote_config_hash::HashRepository,
        remote_config_report::{
            report_remote_config_status_applied, report_remote_config_status_applying,
            report_remote_config_status_error,
        },
    },
    sub_agent::{
        collection::StartedSubAgents, logger::AgentLog,
        values::values_repository::ValuesRepository, NotStartedSubAgent, SubAgentBuilder,
    },
    super_agent::{
        error::AgentError,
        super_agent::{SuperAgent, SuperAgentCallbacks},
    },
};

impl<'a, S, O, HR, SL, HRS, VR> SuperAgent<'a, S, O, HR, SL, HRS, VR>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStore,
    HRS: HashRepository,
    VR: ValuesRepository,
{
    pub(crate) fn valid_remote_config(
        &self,
        mut remote_config: RemoteConfig,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        mut sub_agents: &mut StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
    ) -> Result<(), AgentError> {
        if !remote_config.agent_id.is_super_agent_id() {
            return self.process_sub_agent_remote_config(
                remote_config,
                &mut sub_agents,
                tx,
                opamp_publisher,
            );
        }

        if let Some(opamp_client) = &self.opamp_client {
            return self.process_super_agent_remote_config(
                opamp_client,
                &mut remote_config,
                tx.clone(),
                &mut sub_agents,
                opamp_publisher.clone(),
            );
        } else {
            unreachable!("got remote config without OpAMP being enabled")
        }
    }

    // TODO This call should be moved to on subagent event loop when opamp event remote_config
    // Sub Agent on remote config
    fn process_sub_agent_remote_config(
        &self,
        mut remote_config: RemoteConfig,
        sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
        opamp_publisher: EventPublisher<OpAMPEvent>,
    ) -> Result<(), AgentError> {
        let agent_id = remote_config.agent_id.clone();

        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;
        let remote_config_value = remote_config.get_unique()?;
        // If remote config is empty, we delete the persisted remote config so later the store
        // will load the local config
        if remote_config_value.is_empty() {
            self.remote_values_repo
                .delete_remote(&remote_config.agent_id)?;
        } else {
            // If the config is not valid log we cannot report it to OpAMP as
            // we don't have access to the Sub Agent OpAMP Client here (yet) so
            // for now we mark the remote config as failed and we don't persist it.
            // When the Sub Agent is "recreated" it will report the remote config
            // as failed.
            match AgentValues::try_from(remote_config_value.to_string()) {
                Err(e) => {
                    error!("Error applying Sub Agent remote config: {}", e);
                    remote_config.hash.fail(e.to_string());
                    self.sub_agent_remote_config_hash_repository
                        .save(&remote_config.agent_id, &remote_config.hash)?;
                }
                Ok(agent_values) => self
                    .remote_values_repo
                    .store_remote(&remote_config.agent_id, &agent_values)?,
            }
        }

        let config = self.sub_agents_config_store.load()?;
        let config = config.get(&agent_id)?;
        self.recreate_sub_agent(agent_id, config, tx.clone(), sub_agents, opamp_publisher)?;

        Ok(())
    }
    // Super Agent on remote config
    // Configuration will be reported as applying to OpAMP
    // Valid configuration will be applied and reported as applied to OpAMP
    // Invalid configuration will not be applied and therefore it will not break the execution
    // of the Super Agent. It will be logged and reported as failed to OpAMP
    fn process_super_agent_remote_config(
        &self,
        opamp_client: &O,
        remote_config: &mut RemoteConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        opamp_publisher: EventPublisher<OpAMPEvent>,
    ) -> Result<(), AgentError> {
        info!("Applying SuperAgent remote config");
        report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

        if let Err(err) = self.apply_remote_config(
            remote_config.clone(),
            tx,
            running_sub_agents,
            opamp_publisher,
        ) {
            let error_message = format!("Error applying Super Agent remote config: {}", err);
            error!(error_message);
            Ok(report_remote_config_status_error(
                opamp_client,
                &remote_config.hash,
                error_message,
            )?)
        } else {
            self.set_config_hash_as_applied(&mut remote_config.hash)?;
            Ok(report_remote_config_status_applied(
                opamp_client,
                &remote_config.hash,
            )?)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        config::{
            agent_type::trivial_value::TrivialValue,
            agent_values::AgentValues,
            store::tests::MockSubAgentsConfigStore,
            super_agent_configs::{AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig},
        },
        event::channel::pub_sub,
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            remote_config::{ConfigMap, RemoteConfig},
            remote_config_hash::{test::MockHashRepositoryMock, Hash},
        },
        sub_agent::{
            collection::StartedSubAgents,
            test::{MockStartedSubAgent, MockSubAgentBuilderMock},
            values::values_repository::test::MockRemoteValuesRepositoryMock,
        },
        super_agent::super_agent::SuperAgent,
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    // TODO Move to SubAgent when its event loop is created
    #[test]
    fn receive_sub_agent_opamp_remote_config_existing_sub_agent_should_be_recreated() {
        let (tx, _) = std::sync::mpsc::channel();

        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Given that we have 3 running Sub Agents
        let mut sub_agents = StartedSubAgents::from(HashMap::from([
            (
                AgentID::new("fluent_bit").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (AgentID::new("nrdot").unwrap(), MockStartedSubAgent::new()),
        ]));

        // When we receive a remote config for a Sub Agent
        let sub_agent_id = AgentID::new("infra_agent").unwrap();

        let remote_config = RemoteConfig {
            agent_id: sub_agent_id.clone(),
            hash: Hash::new("sub-agent-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
config_file: /some/path/newrelic-infra.yml
"#
                .to_string(),
            )])),
        };

        // Then hash repository should save the received hash
        sub_agent_hash_repository_mock
            .should_save_hash(&remote_config.agent_id, &remote_config.hash);
        // And values repo should store the received config as values
        let expected_agent_values = AgentValues::new(HashMap::from([(
            "config_file".to_string(),
            TrivialValue::String("/some/path/newrelic-infra.yml".to_string()),
        )]));
        sub_agent_values_repo.should_store_remote(&sub_agent_id, &expected_agent_values);
        // And we reload the config from the Sub Agent Config Store
        let sub_agents_config = SubAgentsConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_rdot"),
                },
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_infra_agent"),
                },
            ),
            (
                AgentID::new("fluent_bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_fluent_bit"),
                },
            ),
        ]));
        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get(&sub_agent_id).should_stop();
        // And the Sub Agent should be re-created
        sub_agent_builder.should_build_running(
            &sub_agent_id,
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("fqn_infra_agent"),
            },
        );

        // Create the Super Agent and run Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(MockStartedOpAMPClientMock::new()),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();

        assert!(super_agent
            .process_sub_agent_remote_config(remote_config, &mut sub_agents, tx, opamp_publisher)
            .is_ok());
    }

    // TODO Move to SubAgent when its event loop is created
    #[test]
    fn receive_sub_agent_remote_deleted_config_should_delete_and_use_local() {
        let (tx, _) = std::sync::mpsc::channel();

        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Given that we have 3 running Sub Agents
        let mut sub_agents = StartedSubAgents::from(HashMap::from([
            (
                AgentID::new("fluent_bit").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (AgentID::new("nrdot").unwrap(), MockStartedSubAgent::new()),
        ]));

        let sub_agent_id = AgentID::new("infra_agent").unwrap();

        // When we receive an empty remote config for a Sub Agent
        let remote_config = RemoteConfig {
            agent_id: sub_agent_id.clone(),
            hash: Hash::new("sub-agent-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([("".to_string(), "".to_string())])),
        };

        // Then hash repository should save the received hash
        sub_agent_hash_repository_mock
            .should_save_hash(&remote_config.agent_id, &remote_config.hash);
        // And config should be deleted
        sub_agent_values_repo.should_delete_remote(&sub_agent_id);
        // And we reload the config from the Sub Agent Config Store
        let sub_agents_config = SubAgentsConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_rdot"),
                },
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_infra_agent"),
                },
            ),
            (
                AgentID::new("fluent_bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_fluent_bit"),
                },
            ),
        ]));
        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get(&sub_agent_id).should_stop();
        // And the Sub Agent should be re-created
        sub_agent_builder.should_build_running(
            &sub_agent_id,
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("fqn_infra_agent"),
            },
        );

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(MockStartedOpAMPClientMock::new()),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        assert!(super_agent
            .process_sub_agent_remote_config(remote_config, &mut sub_agents, tx, opamp_publisher)
            .is_ok());
    }

    // Invalid configuration should be reported to OpAMP as Failed and the Super Agent should
    // not apply it nor crash execution.
    #[test]
    fn super_agent_invalid_remote_config_should_be_reported_as_failed() {
        let (tx, _) = std::sync::mpsc::channel();
        // Mocked services
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let old_sub_agents_config = SubAgentsConfig::default();
        let agent_id = AgentID::new_super_agent_id();
        let mut remote_config = RemoteConfig {
            agent_id,
            hash: Hash::new("this-is-a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                "invalid_yaml_content:{}".to_string(),
            )])),
        };

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        sub_agents_config_store.should_load(&old_sub_agents_config);

        // report failed after trying to unserialize
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "Error applying Super Agent remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct SubAgentsConfig``".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        super_agent
            .process_super_agent_remote_config(
                &started_client,
                &mut remote_config,
                tx,
                &mut running_sub_agents,
                opamp_publisher,
            )
            .unwrap();
    }

    #[test]
    fn super_agent_valid_remote_config_should_be_reported_as_applied() {
        let (tx, _) = std::sync::mpsc::channel();
        // Mocked services
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent_id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        let old_sub_agents_config = SubAgentsConfig::from(HashMap::from([(
            sub_agent_id.clone(),
            SubAgentConfig {
                agent_type: "some_agent_type".into(),
            },
        )]));

        let agent_id = AgentID::new_super_agent_id();
        let mut remote_config = RemoteConfig {
            agent_id,
            hash: Hash::new("this-is-a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([("".to_string(), "agents: {}".to_string())])),
        };

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        sub_agents_config_store.should_load(&old_sub_agents_config);

        // store remote config with empty agents
        sub_agents_config_store.should_store(&SubAgentsConfig::default());

        // persist hash
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config.hash);

        // persist hash after applied
        let mut applied_hash = remote_config.hash.clone();
        applied_hash.apply();
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &applied_hash);

        // Report config status as applied
        let status = RemoteConfigStatus {
            status: Applied as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        super_agent
            .process_super_agent_remote_config(
                &started_client,
                &mut remote_config,
                tx,
                &mut running_sub_agents,
                opamp_publisher,
            )
            .unwrap();
    }
}
