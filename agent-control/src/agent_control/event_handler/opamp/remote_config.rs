use opamp_client::StartedClient;
use tracing::{error, info};

use crate::agent_control::config_storer::loader_storer::{
    AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
    AgentControlDynamicConfigStorer,
};
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::remote_config::validators::DynamicConfigValidator;
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::{
    agent_control::{agent_control::AgentControl, error::AgentError},
    opamp::{hash_repository::HashRepository, remote_config::RemoteConfig},
    sub_agent::{collection::StartedSubAgents, NotStartedSubAgent, SubAgentBuilder},
};

impl<S, O, HR, SL, DV> AgentControl<S, O, HR, SL, DV>
where
    O: StartedClient,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigStorer
        + AgentControlDynamicConfigLoader
        + AgentControlDynamicConfigDeleter,
    DV: DynamicConfigValidator,
{
    // Agent Control on remote config
    // Configuration will be reported as applying to OpAMP
    // Valid configuration will be applied and reported as applied to OpAMP
    pub(crate) fn remote_config(
        &self,
        mut remote_config: RemoteConfig,
        sub_agents: &mut StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let Some(opamp_client) = &self.opamp_client else {
            unreachable!("got remote config without OpAMP being enabled");
        };

        info!(agent_id=%remote_config.agent_id, "Applying remote config");
        OpampRemoteConfigStatus::Applying.report(opamp_client, &remote_config.hash)?;

        match self.apply_remote_agent_control_config(&remote_config, sub_agents) {
            Err(err) => {
                let error_message = format!("Error applying Agent Control remote config: {}", err);
                error!(error_message);
                OpampRemoteConfigStatus::Error(error_message.clone())
                    .report(opamp_client, &remote_config.hash)?;
                Ok(self.report_unhealthy(Unhealthy::new(String::default(), error_message))?)
            }
            Ok(()) => {
                self.set_config_hash_as_applied(&mut remote_config.hash)?;
                OpampRemoteConfigStatus::Applied.report(opamp_client, &remote_config.hash)?;
                opamp_client.update_effective_config()?;
                Ok(self.report_healthy(Healthy::new(String::default()))?)
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::opamp::remote_config::validators::tests::MockDynamicConfigValidatorMock;
    use crate::{
        agent_control::{
            agent_control::AgentControl,
            config::{AgentControlDynamicConfig, AgentID, SubAgentConfig},
            config_storer::loader_storer::tests::MockAgentControlDynamicConfigStore,
        },
        event::channel::pub_sub,
        opamp::{
            client_builder::tests::MockStartedOpAMPClientMock,
            hash_repository::repository::tests::MockHashRepositoryMock,
            remote_config::hash::Hash,
            remote_config::{ConfigurationMap, RemoteConfig},
        },
        sub_agent::{
            collection::StartedSubAgents,
            tests::{MockStartedSubAgent, MockSubAgentBuilderMock},
        },
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    // Invalid configuration should be reported to OpAMP as Failed and the Agent Control should
    // not apply it nor crash execution.
    #[test]
    fn agent_control_invalid_remote_config_should_be_reported_as_failed() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        let hash_repository_mock = Arc::new(MockHashRepositoryMock::new());
        let mut started_client = MockStartedOpAMPClientMock::new();
        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let old_sub_agents_config = AgentControlDynamicConfig::default();
        let agent_id = AgentID::new_agent_control_id();
        let remote_config = RemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                "invalid_yaml_content:{}".to_string(),
            )]))),
        );
        let dynamic_config_validator = MockDynamicConfigValidatorMock::new();

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
            error_message: "Error applying Agent Control remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct AgentControlDynamicConfig``".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        started_client.should_set_unhealthy();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            Some(started_client),
            hash_repository_mock,
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        agent_control
            .remote_config(remote_config, &mut running_sub_agents)
            .unwrap();
    }

    #[test]
    fn agent_control_valid_remote_config_should_be_reported_as_applied() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();
        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent-id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        let old_sub_agents_config = AgentControlDynamicConfig::from(HashMap::from([(
            sub_agent_id.clone(),
            SubAgentConfig {
                agent_type: "namespace/some_agent_type:0.0.1".try_into().unwrap(),
            },
        )]));

        let agent_id = AgentID::new_agent_control_id();
        let remote_config = RemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
                "".to_string(),
                "agents: {}".to_string(),
            )]))),
        );
        let mut dynamic_config_validator = MockDynamicConfigValidatorMock::new();

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);
        started_client.should_update_effective_config(1);

        // load current sub agents config
        sub_agents_config_store.should_load(&old_sub_agents_config);

        // store remote config with empty agents
        sub_agents_config_store.should_store(&AgentControlDynamicConfig::default());

        // persist hash
        hash_repository_mock
            .should_save_hash(&AgentID::new_agent_control_id(), &remote_config.hash);

        // persist hash after applied
        let mut applied_hash = remote_config.hash.clone();
        applied_hash.apply();
        hash_repository_mock.should_save_hash(&AgentID::new_agent_control_id(), &applied_hash);

        // Report config status as applied
        let status = RemoteConfigStatus {
            status: Applied as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        started_client.should_set_healthy();
        let (_opamp_publisher, opamp_consumer) = pub_sub();
        let (agent_control_publisher, _agent_control_consumer) = pub_sub();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();

        dynamic_config_validator
            .expect_validate()
            .times(1)
            .returning(|_| Ok(()));

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
            dynamic_config_validator,
        );

        agent_control
            .remote_config(remote_config, &mut running_sub_agents)
            .unwrap();
    }
}
