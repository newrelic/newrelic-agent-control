use std::sync::mpsc::Sender;

use opamp_client::StartedClient;
use tracing::{error, info};

use crate::event::SubAgentEvent;
use crate::super_agent::store::{
    SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
};
use crate::{
    event::channel::EventPublisher,
    opamp::{
        remote_config::RemoteConfig,
        remote_config_hash::HashRepository,
        remote_config_report::{
            report_remote_config_status_applied, report_remote_config_status_applying,
            report_remote_config_status_error,
        },
    },
    sub_agent::{
        collection::StartedSubAgents, logger::AgentLog, NotStartedSubAgent, SubAgentBuilder,
    },
    super_agent::{
        error::AgentError,
        super_agent::{SuperAgent, SuperAgentCallbacks},
    },
};

impl<'a, S, O, HR, SL> SuperAgent<'a, S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
{
    pub(crate) fn valid_remote_config(
        &self,
        mut remote_config: RemoteConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agents: &mut StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
    ) -> Result<(), AgentError> {
        if let Some(opamp_client) = &self.opamp_client {
            self.process_super_agent_remote_config(
                opamp_client,
                &mut remote_config,
                tx.clone(),
                sub_agents,
                sub_agent_publisher.clone(),
            )
        } else {
            unreachable!("got remote config without OpAMP being enabled")
        }
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
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<(), AgentError> {
        info!("Applying SuperAgent remote config");
        report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

        if let Err(err) = self.apply_remote_config(
            remote_config.clone(),
            tx,
            running_sub_agents,
            sub_agent_publisher,
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
        event::channel::pub_sub,
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            remote_config::{ConfigMap, RemoteConfig},
            remote_config_hash::{test::MockHashRepositoryMock, Hash},
        },
        sub_agent::{
            collection::StartedSubAgents,
            test::{MockStartedSubAgent, MockSubAgentBuilderMock},
        },
        super_agent::{
            config::{AgentID, SubAgentConfig, SubAgentsConfig},
            store::tests::MockSubAgentsConfigStore,
            super_agent::SuperAgent,
        },
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    // Invalid configuration should be reported to OpAMP as Failed and the Super Agent should
    // not apply it nor crash execution.
    #[test]
    fn super_agent_invalid_remote_config_should_be_reported_as_failed() {
        let (tx, _) = std::sync::mpsc::channel();
        // Mocked services
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
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent-id".to_string()).unwrap();
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
