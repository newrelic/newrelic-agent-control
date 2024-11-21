use crate::opamp::remote_config::RemoteConfigError;
use crate::opamp::remote_config_report::report_remote_config_status_applying;
use crate::sub_agent::config_validator::ConfigValidator;
use crate::sub_agent::error::SubAgentError;
use crate::super_agent::config::AgentTypeFQN;
use crate::values::yaml_config::YAMLConfig;
use crate::{
    opamp::{
        hash_repository::HashRepository, remote_config::RemoteConfig,
        remote_config_report::report_remote_config_status_error,
    },
    values::yaml_config_repository::YAMLConfigRepository,
};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;

const ERROR_REMOTE_CONFIG: &str = "Error applying Sub Agent remote config";

/// This method retrieves and stores the remote configuration (hash and values)
/// and sends the status to OpAMP.
/// When the configuration is empty, the values are deleted instead (an empty configuration means that the remote
/// configuration should not apply anymore).
pub fn remote_config<C, CB, HR, Y>(
    remote_config: &mut RemoteConfig,
    maybe_opamp_client: Option<&C>,
    config_validator: &ConfigValidator,
    remote_values_repo: &Y,
    sub_agent_remote_config_hash_repository: &HR,
    agent_type: &AgentTypeFQN,
) -> Result<(), SubAgentError>
where
    C: StartedClient<CB>,
    CB: Callbacks,
    HR: HashRepository,
    Y: YAMLConfigRepository,
{
    let Some(opamp_client) = maybe_opamp_client else {
        unreachable!("got remote config without OpAMP being enabled")
    };

    if let Err(err) = config_validator.validate(agent_type, remote_config) {
        let err = err.into();
        report_remote_config_status_error(
            opamp_client,
            &remote_config.hash,
            format!("{}: {}", ERROR_REMOTE_CONFIG, &err),
        )?;
        return Err(err);
    }

    report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

    if let Err(err) = store_remote_config_hash_and_values(
        remote_config,
        sub_agent_remote_config_hash_repository,
        remote_values_repo,
    ) {
        report_remote_config_status_error(
            opamp_client,
            &remote_config.hash,
            format!("{}: {}", ERROR_REMOTE_CONFIG, &err),
        )?;
        return Err(err);
    }

    Ok(())
}

pub fn store_remote_config_hash_and_values<HS, Y>(
    remote_config: &mut RemoteConfig,
    sub_agent_remote_config_hash_repository: &HS,
    remote_values_repo: &Y,
) -> Result<(), SubAgentError>
where
    HS: HashRepository,
    Y: YAMLConfigRepository,
{
    // Save the configuration hash
    sub_agent_remote_config_hash_repository.save(&remote_config.agent_id, &remote_config.hash)?;
    // The remote configuration can be invalid (checked while deserializing)
    if let Some(err) = remote_config.hash.error_message() {
        return Err(RemoteConfigError::InvalidConfig(remote_config.hash.get(), err).into());
    }
    // Save the configuration values
    match process_remote_config(remote_config) {
        Err(err) => {
            // Store the hash failure if values cannot be obtained from remote config
            remote_config.hash.fail(err.to_string());
            sub_agent_remote_config_hash_repository
                .save(&remote_config.agent_id, &remote_config.hash)?;
            Err(err)
        }
        // Remove previously persisted values when the configuration is empty
        Ok(None) => Ok(remote_values_repo.delete_remote(&remote_config.agent_id)?),
        Ok(Some(agent_values)) => {
            Ok(remote_values_repo.store_remote(&remote_config.agent_id, &agent_values)?)
        }
    }
}

fn process_remote_config(
    remote_config: &RemoteConfig,
) -> Result<Option<YAMLConfig>, SubAgentError> {
    let remote_config_value = remote_config.get_unique()?;

    if remote_config_value.is_empty() {
        return Ok(None);
    }

    Ok(Some(YAMLConfig::try_from(remote_config_value.to_string())?))
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::{remote_config, ERROR_REMOTE_CONFIG};
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::HashRepositoryError;
    use crate::opamp::remote_config::RemoteConfigError;
    use crate::sub_agent::config_validator::{ConfigValidator, ValidatorError};
    use crate::sub_agent::error::SubAgentError;
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::defaults::FQN_NAME_INFRA_AGENT;
    use crate::values::yaml_config::{YAMLConfig, YAMLConfigError};
    use crate::values::yaml_config_repository::YAMLConfigRepositoryError;
    use crate::{
        opamp::{
            client_builder::test::MockStartedOpAMPClientMock,
            hash_repository::repository::test::MockHashRepositoryMock,
            remote_config::{ConfigurationMap, RemoteConfig},
            remote_config_hash::Hash,
        },
        values::yaml_config_repository::test::MockYAMLConfigRepositoryMock,
    };
    use mockall::predicate;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Applying;
    use serde::de::Error;
    use std::collections::HashMap;

    #[test]
    fn test_config_empty() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([("".to_string(), "".to_string())]));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let mut hash_repository = MockHashRepositoryMock::default();
        hash_repository.should_save_hash(&agent_id, &hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
        yaml_config_repository.should_delete_remote(&agent_id);

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: hash.get().into_bytes(),
            error_message: Default::default(),
        });

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert!(remote_config_result.is_ok())
    }

    #[test]
    fn test_config_invalid_agent_values() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "this is not valid yaml".to_string(),
        )]));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::ValuesUnserializeError(YAMLConfigError::FormatError(
            serde_yaml::Error::custom(
                "invalid type: string \"this is not valid yaml\", expected a map",
            ),
        ));

        let mut hash_repository = MockHashRepositoryMock::default();
        // hash should be stored even before finding out it will fail.
        hash_repository.should_save_hash(&agent_id, &hash);

        let mut hash = hash.clone();
        // Fail the hash and report the error
        hash.fail(expected_error.to_string());
        hash_repository.should_save_hash(&agent_id, &hash);

        let yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_config_missing_config() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::new());
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            "empty config map".into(),
        ));

        let mut hash_repository = MockHashRepositoryMock::default();
        // hash should be stored even before finding out it will fail.
        hash_repository.should_save_hash(&agent_id, &hash);

        let mut hash = hash.clone();
        // Fail the hash and report the error
        hash.fail(expected_error.to_string());
        hash_repository.should_save_hash(&agent_id, &hash);

        let yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_config_with_failing_status() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let mut hash = Hash::new(String::from("some-hash"));
        hash.fail("error_message".into());
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), None);

        let expected_error = SubAgentError::RemoteConfigError(RemoteConfigError::InvalidConfig(
            hash.get(),
            hash.error_message().unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::default();
        // hash should be stored even before finding out it will fail.
        hash_repository.should_save_hash(&agent_id, &hash);

        let yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_config_hash_repository_error() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), None);

        let expected_error =
            SubAgentError::from(HashRepositoryError::LoadError("test".to_string()));

        let mut hash_repository = MockHashRepositoryMock::default();
        // hash should be stored even before finding out it will fail.
        hash_repository
            .expect_save()
            .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
            .once()
            .returning(move |_, _| Err(HashRepositoryError::LoadError("test".to_string())));

        let yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_yaml_config_repository_error_on_store() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "some_item: some_value".to_string(),
        )]));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error =
            SubAgentError::from(YAMLConfigRepositoryError::StoreError("store".to_string()));

        let mut hash_repository = MockHashRepositoryMock::default();
        hash_repository.should_save_hash(&agent_id, &hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
        yaml_config_repository
            .expect_store_remote()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(YAMLConfig::new(HashMap::from([(
                    "some_item".into(),
                    "some_value".into(),
                )]))),
            )
            .returning(|_, _| Err(YAMLConfigRepositoryError::StoreError("store".to_string())));

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_config_error_on_delete() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([("".to_string(), "".to_string())]));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error =
            SubAgentError::from(YAMLConfigRepositoryError::DeleteError("delete".to_string()));

        let mut hash_repository = MockHashRepositoryMock::default();
        hash_repository.should_save_hash(&agent_id, &hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
        yaml_config_repository
            .expect_delete_remote()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(|_| Err(YAMLConfigRepositoryError::DeleteError("delete".to_string())));

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Applying config status should be reported
        opamp_client.should_set_remote_config_status(applying_status(&hash));
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
        )
    }

    #[test]
    fn test_config_validation_error() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_type =
            AgentTypeFQN::try_from(format!("namespace/{}:0.0.1", FQN_NAME_INFRA_AGENT).as_str())
                .unwrap();

        let hash = Hash::new(String::from("some-hash"));
        let config_map = ConfigurationMap::new(HashMap::from([(
            "".to_string(),
            "exec: /bin/echo".to_string(),
        )]));
        let mut config = RemoteConfig::new(agent_id.clone(), hash.clone(), Some(config_map));

        let expected_error = SubAgentError::from(ValidatorError::InvalidConfig);

        let hash_repository = MockHashRepositoryMock::default();

        let yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        let mut opamp_client: MockStartedOpAMPClientMock<
            AgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockStartedOpAMPClientMock::new();
        // Failed config status should be reported
        opamp_client.should_set_remote_config_status(failing_status(&hash, &expected_error));

        let remote_config_result = remote_config(
            &mut config,
            Some(&opamp_client),
            &ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            &yaml_config_repository,
            &hash_repository,
            &agent_type,
        );

        assert_eq!(
            expected_error.to_string(),
            remote_config_result.unwrap_err().to_string()
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
