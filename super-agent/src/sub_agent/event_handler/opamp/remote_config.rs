use crate::agent_type::agent_values::AgentValues;
use crate::event::SubAgentEvent;
use crate::opamp::remote_config::RemoteConfigError;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::SubAgentCallbacks;
use crate::{
    opamp::{
        hash_repository::HashRepository, remote_config::RemoteConfig,
        remote_config_report::report_remote_config_status_error,
    },
    sub_agent::values::values_repository::ValuesRepository,
};
use opamp_client::StartedClient;

const ERROR_REMOTE_CONFIG: &str = "Error applying Sub Agent remote config";

impl<C, S, R> EventProcessor<C, S, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    S: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn remote_config(&self, remote_config: RemoteConfig) -> Result<(), SubAgentError> {
        let Some(opamp_client) = &self.maybe_opamp_client else {
            unreachable!("got remote config without OpAMP being enabled")
        };

        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;

        // Remote config could already been in failure status.
        if let Some(e) = remote_config.hash.error_message() {
            let err = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
                remote_config.hash.get(),
                e,
            ));
            report_remote_config_status_error(
                opamp_client,
                &remote_config.hash,
                format!("{}: {}", ERROR_REMOTE_CONFIG, &err),
            )?;
            return Err(err);
        }

        let mut remote_config = remote_config;
        match self.process_remote_config(&remote_config) {
            Err(err) => {
                remote_config.hash.fail(err.to_string());
                self.sub_agent_remote_config_hash_repository
                    .save(&remote_config.agent_id, &remote_config.hash)?;
                report_remote_config_status_error(
                    opamp_client,
                    &remote_config.hash,
                    format!("{}: {}", ERROR_REMOTE_CONFIG, &err),
                )?;
                return Err(err);
            }
            // If remote config is empty, we delete the persisted remote config so later the store
            // will load the local config
            Ok(None) => self
                .remote_values_repo
                .delete_remote(&remote_config.agent_id)?,

            Ok(Some(agent_values)) => self
                .remote_values_repo
                .store_remote(&remote_config.agent_id, &agent_values)?,
        }

        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::ConfigUpdated(self.agent_id()))?)
    }
    pub(crate) fn process_remote_config(
        &self,
        remote_config: &RemoteConfig,
    ) -> Result<Option<AgentValues>, SubAgentError> {
        let remote_config_value = remote_config.get_unique()?;

        if remote_config_value.is_empty() {
            return Ok(None);
        }

        Ok(Some(AgentValues::try_from(
            remote_config_value.to_string(),
        )?))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ERROR_REMOTE_CONFIG;
    use crate::agent_type::agent_values::{AgentValues, AgentValuesError};
    use crate::event::SubAgentEvent::ConfigUpdated;
    use crate::opamp::remote_config::RemoteConfigError;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::EventProcessor;
    use crate::super_agent::config::AgentID;
    use crate::{
        event::channel::pub_sub,
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            hash_repository::repository::test::MockHashRepositoryMock,
            remote_config::{ConfigMap, RemoteConfig},
            remote_config_hash::Hash,
        },
        sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock,
    };
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use serde::de::Error;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_config_success() {
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
            &AgentValues::new(HashMap::from([("some_item".into(), "some_value".into())])),
        );

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

        let event_processor = EventProcessor::new(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        event_processor.remote_config(remote_config).unwrap();

        let expected_event = ConfigUpdated(agent_id);
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }

    #[test]
    fn test_config_with_empty_agents() {
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

        // Expect to remove current remote config when receiving an empty agents remote config.
        values_repository.should_delete_remote(&agent_id);

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

        let event_processor = EventProcessor::new(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        event_processor.remote_config(remote_config).unwrap();

        let expected_event = ConfigUpdated(agent_id);
        assert_eq!(
            expected_event,
            sub_agent_consumer
                .as_ref()
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
        );
    }

    #[test]
    fn test_config_invalid_agent_values() {
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

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::ValuesUnserializeError(AgentValuesError::FormatError(
            serde_yaml::Error::custom(
                "invalid type: string \"this is not valid yaml\", expected a map",
            ),
        ));
        hash_repository.should_save_hash(&agent_id, &hash);
        // Fail the hash and report the error
        hash.fail(expected_error.to_string());
        hash_repository.should_save_hash(&agent_id, &hash);

        // report failed config
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: format!("{}: {}", ERROR_REMOTE_CONFIG, expected_error),
        };
        opamp_client.should_set_remote_config_status(status);

        let event_processor = EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    #[test]
    fn test_config_missing_config() {
        let mut opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let config_map = ConfigMap::new(HashMap::new());

        let mut hash = Hash::new(String::from("some-hash"));

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            "empty config map".into(),
        ));
        hash_repository.should_save_hash(&agent_id, &hash);
        // Fail the hash and report the error
        hash.fail(expected_error.to_string());
        hash_repository.should_save_hash(&agent_id, &hash);

        // report failed config
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: format!("{}: {}", ERROR_REMOTE_CONFIG, expected_error),
        };
        opamp_client.should_set_remote_config_status(status);

        let event_processor = EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    #[test]
    fn test_config_with_failing_status() {
        let mut opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, _sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let mut hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();

        let mut hash = Hash::new(String::from("some-hash"));
        hash.fail("error_message".into());

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), None);

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            hash.error_message().unwrap(),
        ));

        hash_repository.should_save_hash(&agent_id, &hash);

        // report failed config
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: format!("{}: {}", ERROR_REMOTE_CONFIG, expected_error),
        };
        opamp_client.should_set_remote_config_status(status);

        let event_processor = EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer.into(),
            sub_agent_internal_consumer,
            Some(opamp_client),
            Arc::new(hash_repository),
            Arc::new(values_repository),
        );

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }
}
