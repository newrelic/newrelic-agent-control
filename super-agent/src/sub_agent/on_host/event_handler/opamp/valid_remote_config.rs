use crate::event::SubAgentEvent;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::on_host::event_processor::EventProcessor;
use crate::sub_agent::SubAgentCallbacks;
use crate::{
    config::agent_values::AgentValues,
    opamp::{
        remote_config::RemoteConfig, remote_config_hash::HashRepository,
        remote_config_report::report_remote_config_status_error,
    },
    sub_agent::values::values_repository::ValuesRepository,
};
use opamp_client::StartedClient;

impl<C, S, R> EventProcessor<C, S, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    S: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn valid_remote_config(
        &self,
        remote_config: RemoteConfig,
    ) -> Result<(), SubAgentError> {
        if self.maybe_opamp_client.is_some() {
            self.process_sub_agent_remote_config(remote_config)
        } else {
            unreachable!("got remote config without OpAMP being enabled")
        }
    }

    fn process_sub_agent_remote_config(
        &self,
        mut remote_config: RemoteConfig,
    ) -> Result<(), SubAgentError> {
        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;
        let remote_config_value = remote_config.get_unique()?;

        // If remote config is empty, we delete the persisted remote config so later the store
        // will load the local config
        if remote_config_value.is_empty() {
            self.remote_values_repo
                .delete_remote(&remote_config.agent_id)?;
        } else {
            match AgentValues::try_from(remote_config_value.to_string()) {
                // Invalid config will persist hash as invalid and report config status error to OpAMP
                Err(err) => {
                    remote_config.hash.fail(err.to_string());
                    self.sub_agent_remote_config_hash_repository
                        .save(&remote_config.agent_id, &remote_config.hash)?;

                    report_remote_config_status_error(
                        self.maybe_opamp_client.as_ref().unwrap(),
                        &remote_config.hash,
                        format!("Error applying Sub Agent remote config: {}", err),
                    )?;
                    return Err(err.into());
                }
                Ok(agent_values) => self
                    .remote_values_repo
                    .store_remote(&remote_config.agent_id, &agent_values)?,
            }
        }

        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::ConfigUpdated(remote_config.agent_id.clone()))?)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::event::SubAgentEvent::ConfigUpdated;
    use crate::sub_agent::on_host::event_processor::EventProcessor;
    use crate::{
        config::{
            agent_type::trivial_value::TrivialValue, agent_values::AgentValues,
            super_agent_configs::AgentID,
        },
        event::channel::pub_sub,
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            remote_config::{ConfigMap, RemoteConfig},
            remote_config_hash::{test::MockHashRepositoryMock, Hash},
        },
        sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock,
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use serde_yaml::{Mapping, Value};

    #[test]
    fn test_valid_config_not_empty() {
        let opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let mut values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        hash_repository.should_save_hash(&agent_id, &hash);
        values_repository.should_store_remote(
            &agent_id,
            &AgentValues::new(Value::Mapping(Mapping::from_iter([(
                "some_item".into(),
                "some_value".into(),
            )]))),
        );

        let remote_config = RemoteConfig {
            config_map,
            hash,
            agent_id: agent_id.clone(),
        };

        let event_processor = EventProcessor::new(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        event_processor.valid_remote_config(remote_config).unwrap();

        let expected_event = ConfigUpdated(agent_id);
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }

    #[test]
    fn test_valid_config_empty() {
        let opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let mut values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([("".to_string(), "".to_string())]));

        hash_repository.should_save_hash(&agent_id, &hash);
        values_repository.should_delete_remote(&agent_id);

        let remote_config = RemoteConfig {
            config_map,
            hash,
            agent_id: agent_id.clone(),
        };

        let event_processor = EventProcessor::new(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        event_processor.valid_remote_config(remote_config).unwrap();

        let expected_event = ConfigUpdated(agent_id);
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }

    #[test]
    fn test_valid_config_invalid_values() {
        let mut opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "this is not valid yaml".to_string(),
        )]));

        let mut hash = Hash::new(String::from("some-hash"));

        let remote_config = RemoteConfig {
            config_map,
            hash: hash.clone(),
            agent_id: agent_id.clone(),
        };

        hash_repository.should_save_hash(&agent_id, &hash);
        // Fail the hash and report the error
        hash.fail(String::from("invalid agent values format: `invalid type: string \"this is not valid yaml\", expected a map`"));
        hash_repository.should_save_hash(&agent_id, &hash);

        // report failed config
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: "Error applying Sub Agent remote config: invalid agent values format: `invalid type: string \"this is not valid yaml\", expected a map`".to_string(),
        };
        opamp_client.should_set_remote_config_status(status);

        let event_processor = EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        let res = event_processor.valid_remote_config(remote_config);
        assert_eq!(
            "sub agent values error: `invalid agent values format: `invalid type: string \"this is not valid yaml\", expected a map``",
            res.unwrap_err().to_string()
        );
    }
}
