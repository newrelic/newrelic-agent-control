use crate::agent_type::agent_values::AgentValues;
use crate::event::SubAgentEvent;
use crate::opamp::remote_config::RemoteConfigError;
use crate::opamp::remote_config_report::report_remote_config_status_applying;
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
    /// This method retrieves, stores the remote configuration (hash and values) and publish an event super-agent event
    /// in order that the super-agent handles it.
    /// When the configuration is empty, the values are deleted instead (an empty configuration means that the remote
    /// configuration should not apply anymore).
    pub(crate) fn remote_config(&self, remote_config: RemoteConfig) -> Result<(), SubAgentError> {
        let Some(opamp_client) = &self.maybe_opamp_client else {
            unreachable!("got remote config without OpAMP being enabled")
        };

        report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

        let mut remote_config = remote_config;
        if let Err(err) = self.store_remote_config_hash_and_values(&mut remote_config) {
            report_remote_config_status_error(
                opamp_client,
                &remote_config.hash,
                format!("{}: {}", ERROR_REMOTE_CONFIG, &err),
            )?;
            return Err(err);
        }

        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::ConfigUpdated(self.agent_id()))?)
    }

    fn store_remote_config_hash_and_values(
        &self,
        remote_config: &mut RemoteConfig,
    ) -> Result<(), SubAgentError> {
        // Save the configuration hash
        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;
        // The remote configuration can be invalid (checked while deserializing)
        if let Some(err) = remote_config.hash.error_message() {
            return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
        }
        // Save the configuration values
        match Self::process_remote_config(remote_config) {
            Err(err) => {
                // Store the hash failure if values cannot be obtained from remote config
                remote_config.hash.fail(err.to_string());
                self.sub_agent_remote_config_hash_repository
                    .save(&remote_config.agent_id, &remote_config.hash)?;
                Err(err)
            }
            // Remove previously persisted values when the configuration is empty
            Ok(None) => Ok(self
                .remote_values_repo
                .delete_remote(&remote_config.agent_id)?),
            Ok(Some(agent_values)) => Ok(self
                .remote_values_repo
                .store_remote(&remote_config.agent_id, &agent_values)?),
        }
    }

    fn process_remote_config(
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
    use crate::event::channel::EventConsumer;
    use crate::event::SubAgentEvent::{self, ConfigUpdated};
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::hash_repository::HashRepositoryError;
    use crate::opamp::remote_config::RemoteConfigError;
    use crate::sub_agent::error::SubAgentError;
    use crate::sub_agent::event_processor::EventProcessor;
    use crate::sub_agent::values::ValuesRepositoryError;
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
    use mockall::predicate;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses;
    use serde::de::Error;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_config_success() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        let (event_processor, sub_agent_consumer) =
            setup_testing_event_processor(&agent_id, |mocks| {
                mocks.hash_repository.should_save_hash(&agent_id, &hash);
                mocks.values_repository.should_store_remote(
                    &agent_id,
                    &AgentValues::new(HashMap::from([("some_item".into(), "some_value".into())])),
                );

                // Applying status should be reported
                mocks
                    .opamp_client
                    .should_set_remote_config_status(applying_status(&hash));
            });

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

        event_processor.remote_config(remote_config).unwrap();

        let expected_event = ConfigUpdated(agent_id);
        assert_eq!(expected_event, sub_agent_consumer.as_ref().recv().unwrap());
    }

    #[test]
    fn test_config_empty() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([("".to_string(), "".to_string())]));

        let (event_processor, sub_agent_consumer) =
            setup_testing_event_processor(&agent_id, |mocks| {
                mocks.hash_repository.should_save_hash(&agent_id, &hash);

                // Expect to remove current remote config when receiving an empty agents remote config.
                mocks.values_repository.should_delete_remote(&agent_id);
                // Applying status should be reported
                mocks
                    .opamp_client
                    .should_set_remote_config_status(applying_status(&hash));
            });

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

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
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "this is not valid yaml".to_string(),
        )]));

        let hash = Hash::new(String::from("some-hash"));

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::ValuesUnserializeError(AgentValuesError::FormatError(
            serde_yaml::Error::custom(
                "invalid type: string \"this is not valid yaml\", expected a map",
            ),
        ));

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            // hash should be stored even before finding out it will fail.
            mocks.hash_repository.should_save_hash(&agent_id, &hash);
            // Once the error is detected the failing version should be persisted
            let mut hash = hash.clone();
            // Fail the hash and report the error
            hash.fail(expected_error.to_string());
            mocks.hash_repository.should_save_hash(&agent_id, &hash);

            // Applying config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

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
        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let config_map = ConfigMap::new(HashMap::new());

        let hash = Hash::new(String::from("some-hash"));

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            "empty config map".into(),
        ));

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            // hash should be stored even before finding out it will fail.
            mocks.hash_repository.should_save_hash(&agent_id, &hash);
            // Once the error is detected the failing version should be persisted
            let mut hash = hash.clone();
            hash.fail(expected_error.to_string());
            mocks.hash_repository.should_save_hash(&agent_id, &hash);

            // Applying config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

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
        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();

        let mut hash = Hash::new(String::from("some-hash"));
        hash.fail("error_message".into());

        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), None);

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            hash.error_message().unwrap(),
        ));

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            mocks.hash_repository.should_save_hash(&agent_id, &hash);
            // Applying config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    #[test]
    fn test_config_hash_repository_error() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let remote_config = RemoteConfig::new(agent_id.clone(), hash.clone(), None);

        let expected_error = SubAgentError::from(HashRepositoryError::Generic);

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            mocks
                .hash_repository
                .expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Err(HashRepositoryError::Generic));

            // Applying config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    #[test]
    fn test_config_values_repository_error_on_store() {
        // Event's config
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));

        let expected_error = SubAgentError::from(ValuesRepositoryError::Generic);

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            mocks.hash_repository.should_save_hash(&agent_id, &hash);
            mocks
                .values_repository
                .expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(AgentValues::new(HashMap::from([(
                        "some_item".into(),
                        "some_value".into(),
                    )]))),
                )
                .returning(|_, _| Err(ValuesRepositoryError::Generic));

            // Applying status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    #[test]
    fn test_config_error_on_delete() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigMap::new(HashMap::from([("".to_string(), "".to_string())]));

        let expected_error = SubAgentError::from(ValuesRepositoryError::Generic);

        let (event_processor, _) = setup_testing_event_processor(&agent_id, |mocks| {
            mocks.hash_repository.should_save_hash(&agent_id, &hash);

            // Failure removing the values (removing since the configuration is empty)
            mocks
                .values_repository
                .expect_delete_remote()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Err(ValuesRepositoryError::Generic));

            // Applying status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(applying_status(&hash));
            // Failed config status should be reported
            mocks
                .opamp_client
                .should_set_remote_config_status(failing_status(&hash, &expected_error));
        });

        let remote_config = RemoteConfig::new(agent_id.clone(), hash, Some(config_map));

        assert_eq!(
            expected_error.to_string(),
            event_processor
                .remote_config(remote_config)
                .unwrap_err()
                .to_string()
        )
    }

    /// Helper struct to group test mocks
    struct TestMocks {
        hash_repository: MockHashRepositoryMock,
        values_repository: MockRemoteValuesRepositoryMock,
        opamp_client: MockStartedOpAMPClientMock<AgentCallbacks>,
    }

    type TestingEventProcessor = EventProcessor<
        MockStartedOpAMPClientMock<AgentCallbacks>,
        MockHashRepositoryMock,
        MockRemoteValuesRepositoryMock,
    >;

    /// Setups and event_processor for testing `remote_config`, given the provided agent_type and mock expectations.
    /// It returns the event_processor and the corresponding sub-agent consumer.
    fn setup_testing_event_processor<F>(
        agent_id: &AgentID,
        expectations_fn: F,
    ) -> (TestingEventProcessor, EventConsumer<SubAgentEvent>)
    where
        F: Fn(&mut TestMocks),
    {
        let opamp_client = MockStartedOpAMPClientMock::new();
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let (_sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (_sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();
        let hash_repository = MockHashRepositoryMock::default();
        let values_repository = MockRemoteValuesRepositoryMock::default();

        let mut test_values = TestMocks {
            hash_repository,
            values_repository,
            opamp_client,
        };

        expectations_fn(&mut test_values);

        (
            EventProcessor::new(
                agent_id.clone(),
                sub_agent_publisher,
                sub_agent_opamp_consumer.into(),
                sub_agent_internal_consumer,
                Some(test_values.opamp_client),
                Arc::new(test_values.hash_repository),
                Arc::new(test_values.values_repository),
            ),
            sub_agent_consumer,
        )
    }

    fn applying_status(hash: &Hash) -> RemoteConfigStatus {
        RemoteConfigStatus {
            status: RemoteConfigStatuses::Applying as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        }
    }

    fn failing_status(hash: &Hash, error: &SubAgentError) -> RemoteConfigStatus {
        RemoteConfigStatus {
            status: RemoteConfigStatuses::Failed as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: format!("{}: {}", ERROR_REMOTE_CONFIG, error),
        }
    }
}
