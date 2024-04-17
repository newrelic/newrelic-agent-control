use opamp_client::StartedClient;
use tracing::{error, info};

use crate::event::{SubAgentEvent, SuperAgentEvent};
use crate::super_agent::config_storer::storer::{
    SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader, SuperAgentDynamicConfigStorer,
};
use crate::{
    event::channel::EventPublisher,
    opamp::{
        hash_repository::HashRepository,
        remote_config::RemoteConfig,
        remote_config_report::{
            report_remote_config_status_applied, report_remote_config_status_applying,
            report_remote_config_status_error,
        },
    },
    sub_agent::{collection::StartedSubAgents, NotStartedSubAgent, SubAgentBuilder},
    super_agent::{
        error::AgentError,
        super_agent::{SuperAgent, SuperAgentCallbacks},
    },
};

impl<S, O, HR, SL> SuperAgent<S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SuperAgentDynamicConfigStorer
        + SuperAgentDynamicConfigLoader
        + SuperAgentDynamicConfigDeleter,
{
    // Super Agent on remote config
    // Configuration will be reported as applying to OpAMP
    // Valid configuration will be applied and reported as applied to OpAMP
    pub(crate) fn remote_config(
        &self,
        mut remote_config: RemoteConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agents: &mut StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let Some(opamp_client) = &self.opamp_client else {
            unreachable!("got remote config without OpAMP being enabled");
        };

        info!("Applying SuperAgent remote config");
        report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

        match self.apply_remote_super_agent_config(&remote_config, sub_agents, sub_agent_publisher)
        {
            Err(err) => {
                let error_message = format!("Error applying Super Agent remote config: {}", err);
                error!(error_message);
                report_remote_config_status_error(
                    opamp_client,
                    &remote_config.hash,
                    error_message.clone(),
                )?;
                self.report_unhealthy(error_message.clone())?;
                self.super_agent_publisher
                    .publish(SuperAgentEvent::SuperAgentBecameUnhealthy(error_message))?;
                Ok(())
            }
            Ok(()) => {
                self.set_config_hash_as_applied(&mut remote_config.hash)?;
                report_remote_config_status_applied(opamp_client, &remote_config.hash)?;
                self.report_healthy()?;
                self.super_agent_publisher
                    .publish(SuperAgentEvent::SuperAgentBecameHealthy)?;
                Ok(())
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

    use crate::{
        event::channel::pub_sub,
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            hash_repository::repository::test::MockHashRepositoryMock,
            remote_config::{ConfigMap, RemoteConfig},
            remote_config_hash::Hash,
        },
        sub_agent::{
            collection::StartedSubAgents,
            test::{MockStartedSubAgent, MockSubAgentBuilderMock},
        },
        super_agent::{
            config::{AgentID, SubAgentConfig, SuperAgentDynamicConfig},
            config_storer::storer::tests::MockSuperAgentDynamicConfigStore,
            super_agent::SuperAgent,
        },
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    // Invalid configuration should be reported to OpAMP as Failed and the Super Agent should
    // not apply it nor crash execution.
    #[test]
    fn super_agent_invalid_remote_config_should_be_reported_as_failed() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
        let hash_repository_mock = Arc::new(MockHashRepositoryMock::new());
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let old_sub_agents_config = SuperAgentDynamicConfig::default();
        let agent_id = AgentID::new_super_agent_id();
        let remote_config = RemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigMap::new(HashMap::from([(
                "".to_string(),
                "invalid_yaml_content:{}".to_string(),
            )]))),
        );

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
            error_message: "Error applying Super Agent remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct SuperAgentDynamicConfig``".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        started_client.should_set_unhealthy();

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            super_agent_publisher,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        super_agent
            .remote_config(remote_config, opamp_publisher, &mut running_sub_agents)
            .unwrap();
    }

    #[test]
    fn super_agent_valid_remote_config_should_be_reported_as_applied() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSuperAgentDynamicConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent-id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        let old_sub_agents_config = SuperAgentDynamicConfig::from(HashMap::from([(
            sub_agent_id.clone(),
            SubAgentConfig {
                agent_type: "some_agent_type".into(),
            },
        )]));

        let agent_id = AgentID::new_super_agent_id();
        let remote_config = RemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigMap::new(HashMap::from([(
                "".to_string(),
                "agents: {}".to_string(),
            )]))),
        );

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
        sub_agents_config_store.should_store(&SuperAgentDynamicConfig::default());

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

        started_client.should_set_healthy();

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            Arc::new(hash_repository_mock),
            sub_agent_builder,
            sub_agents_config_store,
            super_agent_publisher,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        super_agent
            .remote_config(remote_config, opamp_publisher, &mut running_sub_agents)
            .unwrap();
    }
}
