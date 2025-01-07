use opamp_client::StartedClient;
use tracing::{error, info};

use crate::agent_control::config_storer::loader_storer::{
    AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
    AgentControlDynamicConfigStorer,
};
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::{
    agent_control::{
        agent_control::{AgentControl, AgentControlCallbacks},
        error::AgentError,
    },
    opamp::remote_config::RemoteConfig,
    sub_agent::{collection::StartedSubAgents, NotStartedSubAgent, SubAgentBuilder},
};

impl<S, O, SL, G> AgentControl<S, O, SL, G>
where
    G: EffectiveConfigLoader,
    O: StartedClient<AgentControlCallbacks<G>>,
    S: SubAgentBuilder,
    SL: AgentControlDynamicConfigStorer
        + AgentControlDynamicConfigLoader
        + AgentControlDynamicConfigDeleter,
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

        info!("Applying AgentControl remote config");
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
                remote_config.hash.apply();
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

    use crate::agent_control::agent_control::AgentControlCallbacks;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
    use crate::{
        agent_control::{
            agent_control::AgentControl,
            config::{AgentControlDynamicConfig, AgentID, SubAgentConfig},
            config_storer::loader_storer::tests::MockAgentControlDynamicConfigStore,
        },
        event::channel::pub_sub,
        opamp::{
            client_builder::tests::MockStartedOpAMPClientMock,
            remote_config::hash::Hash,
            remote_config::{ConfigurationMap, RemoteConfig},
        },
        sub_agent::{
            collection::StartedSubAgents,
            tests::{MockStartedSubAgent, MockSubAgentBuilderMock},
        },
    };
    use mockall::predicate;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    // Invalid configuration should be reported to OpAMP as Failed and the Agent Control should
    // not apply it nor crash execution.
    #[test]
    fn agent_control_invalid_remote_config_should_be_reported_as_failed() {
        // Mocked services
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockAgentControlDynamicConfigStore::new();
        let mut started_client = MockStartedOpAMPClientMock::<
            AgentControlCallbacks<MockEffectiveConfigLoaderMock>,
        >::new();
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

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        sub_agents_config_store
            .expect_load()
            .once()
            .return_once(move || Ok(old_sub_agents_config.clone()));

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
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
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
        let mut started_client = MockStartedOpAMPClientMock::<
            AgentControlCallbacks<MockEffectiveConfigLoaderMock>,
        >::new();
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
        let mut remote_config = RemoteConfig::new(
            agent_id,
            Hash::new("this-is-a-hash".to_string()),
            Some(ConfigurationMap::new(HashMap::from([(
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
        started_client.should_update_effective_config(1);

        // load current sub agents config
        sub_agents_config_store
            .expect_load()
            .once()
            .return_once(move || Ok(old_sub_agents_config.clone()));

        // persist hash after applied
        remote_config.hash.apply();

        let remote_config_status: AgentRemoteConfigStatus =
            remote_config.clone().try_into().unwrap();
        sub_agents_config_store
            .expect_store()
            .once()
            .with(predicate::eq(remote_config_status))
            .return_once(|_| Ok(()));

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

        // Create the Agent Control and rub Sub Agents
        let agent_control = AgentControl::new(
            Some(started_client),
            sub_agent_builder,
            Arc::new(sub_agents_config_store),
            agent_control_publisher,
            sub_agent_publisher,
            pub_sub().1,
            Some(opamp_consumer),
        );

        agent_control
            .remote_config(remote_config, &mut running_sub_agents)
            .unwrap();
    }
}
