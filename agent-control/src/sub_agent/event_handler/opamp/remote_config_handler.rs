use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::RemoteConfig;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::YAMLConfig;
use thiserror::Error;
use tracing::{debug, error};

type ErrorMessage = String;

#[derive(Debug, Error)]
pub enum RemoteConfigHandlerError {
    #[error("remote configuration with validation errors: {0}")]
    Validation(ErrorMessage),
    #[error("remote configuration cannot be loaded: {0}")]
    RemoteConfigLoad(String),
    #[error("remote configuration with invalid values: {0}")]
    InvalidValues(String),
}

/// Defines how to validate and process a remote configuration and obtain the corresponding yaml configuration.
pub trait RemoteConfigHandler {
    fn handle(
        &self,
        agent_identity: AgentIdentity,
        config: &RemoteConfig,
    ) -> Result<Option<YAMLConfig>, RemoteConfigHandlerError>;
}

pub struct AgentRemoteConfigHandler<V> {
    remote_config_validators: Vec<V>,
}

impl<V> AgentRemoteConfigHandler<V>
where
    V: RemoteConfigValidator,
{
    pub fn new(remote_config_validators: Vec<V>) -> Self {
        AgentRemoteConfigHandler {
            remote_config_validators,
        }
    }
}

impl<V> RemoteConfigHandler for AgentRemoteConfigHandler<V>
where
    V: RemoteConfigValidator,
{
    /// Handles the remote configuration received by the OpAMP client and returns the corresponding yaml configuration
    /// or an error if the configuration is invalid according to the configured validators.
    fn handle(
        &self,
        agent_identity: AgentIdentity,
        config: &RemoteConfig,
    ) -> Result<Option<YAMLConfig>, RemoteConfigHandlerError> {
        // Errors here will cause the sub-agent to continue running with the previous configuration.
        // The supervisor won't be recreated.
        if let Some(err_msg) = config.hash.error_message() {
            return Err(RemoteConfigHandlerError::RemoteConfigLoad(err_msg));
        }
        for validator in &self.remote_config_validators {
            if let Err(error_msg) = validator.validate(&agent_identity, config) {
                debug!(
                    hash = &config.hash.get(),
                    "Invalid remote configuration: {error_msg}"
                );
                return Err(RemoteConfigHandlerError::Validation(error_msg.to_string()));
            }
        }
        extract_remote_config_values(config)
    }
}

/// Extracts the remote configuration values and parses them to [YAMLConfig], if the values are empty it returns None.
fn extract_remote_config_values(
    remote_config: &RemoteConfig,
) -> Result<Option<YAMLConfig>, RemoteConfigHandlerError> {
    let remote_config_value = remote_config.get_unique().map_err(|err| {
        RemoteConfigHandlerError::InvalidValues(format!(
            "could not load remote configuration values: {err}"
        ))
    })?;

    if remote_config_value.is_empty() {
        return Ok(None);
    }

    let yaml_config = YAMLConfig::try_from(remote_config_value.to_string())
        .map_err(|err| RemoteConfigHandlerError::InvalidValues(err.to_string()))?;

    Ok(Some(yaml_config))
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;

    use super::{AgentRemoteConfigHandler, RemoteConfigHandler, RemoteConfigHandlerError};
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidator;
    use crate::opamp::remote_config::{ConfigurationMap, RemoteConfig};
    use crate::sub_agent::identity::tests::test_agent_identity;
    use crate::sub_agent::identity::AgentIdentity;
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use mockall::mock;
    use predicates::prelude::predicate;
    use rstest::rstest;

    mock! {
        pub RemoteConfigHandler {}

        impl RemoteConfigHandler for RemoteConfigHandler{
            fn handle(
                &self,
                agent_identity: AgentIdentity,
                config: &RemoteConfig
            ) -> Result<Option<YAMLConfig>, RemoteConfigHandlerError>;
        }
    }

    impl MockRemoteConfigHandler {
        pub fn should_handle(
            &mut self,
            agent_identity: AgentIdentity,
            config: RemoteConfig,
            yaml_config: Option<YAMLConfig>,
        ) {
            self.expect_handle()
                .once()
                .with(predicate::eq(agent_identity), predicate::eq(config))
                .return_once(|_, _| Ok(yaml_config));
        }
    }

    #[test]
    fn test_agent_remote_config_handler_config_with_previous_errors() {
        let agent_identity = test_agent_identity();
        // The hash had some previous errors
        let hash = Hash::failed("some-hash".into(), "some error".into());
        let remote_config = RemoteConfig::new(
            agent_identity.id.clone(),
            hash,
            Some(ConfigurationMap::default()),
        );

        let handler = AgentRemoteConfigHandler::<MockRemoteConfigValidator>::new(Vec::new());
        let result = handler.handle(agent_identity, &remote_config);
        assert_matches!(result, Err(RemoteConfigHandlerError::RemoteConfigLoad(s)) => {
            assert_eq!(s, "some error".to_string());
        });
    }

    #[test]
    fn test_agent_remote_config_handler_config_validation_error() {
        let agent_identity = test_agent_identity();

        let hash = Hash::new("some-hash".into());
        let remote_config = RemoteConfig::new(
            agent_identity.id.clone(),
            hash,
            Some(ConfigurationMap::default()),
        );

        let mut validator1 = MockRemoteConfigValidator::new();
        let mut validator2 = MockRemoteConfigValidator::new();
        let mut validator3 = MockRemoteConfigValidator::new();

        validator1.should_validate(&agent_identity, &remote_config, Ok(()));
        validator2.should_validate(
            &agent_identity,
            &remote_config,
            Err("validation2 error".into()),
        );
        validator3.expect_validate().never();

        let handler = AgentRemoteConfigHandler::new(vec![validator1, validator2, validator3]);

        let result = handler.handle(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigHandlerError::Validation(s)) => {
            assert_eq!(s, "validation2 error".to_string());
        });
    }

    #[rstest]
    #[case::mutiple_configs(
        r#"{"config1": "{\"key\": \"value\"}", "config2": "{\"key\": \"value\"}"}"#
    )]
    #[case::invalid_yaml_config_single_value(r#"{"config": "single-value"}"#)]
    #[case::invalid_yaml_config_array(r#"{"config": "[1, 2, 3]"}"#)]
    fn test_agent_remote_config_handler_config_invalid_values(#[case] config: &str) {
        let agent_identity = test_agent_identity();

        let hash = Hash::new("some-hash".into());
        let config_map =
            ConfigurationMap::new(serde_json::from_str::<HashMap<String, String>>(config).unwrap());
        let remote_config = RemoteConfig::new(agent_identity.id.clone(), hash, Some(config_map));

        let handler = AgentRemoteConfigHandler::<MockRemoteConfigValidator>::new(Vec::new());

        let result = handler.handle(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigHandlerError::InvalidValues(_)));
    }

    #[test]
    fn test_agent_remote_config_handler_some_config() {
        let agent_identity = test_agent_identity();

        let hash = Hash::new("some-hash".into());
        let config_map = ConfigurationMap::new(
            serde_json::from_str::<HashMap<String, String>>(
                r#"{"config": "{\"key\": \"value\"}"}"#,
            )
            .unwrap(),
        );
        let remote_config = RemoteConfig::new(agent_identity.id.clone(), hash, Some(config_map));

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &remote_config, Ok(()));

        let handler = AgentRemoteConfigHandler::new(vec![validator]);

        let expected: YAMLConfig = serde_yaml::from_str("key: value").unwrap();

        let result = handler.handle(agent_identity.clone(), &remote_config);
        assert_matches!(result, Ok(Some(yaml_config)) => {
            assert_eq!(yaml_config, expected);
        });
    }

    #[test]
    fn test_agent_remote_config_handler_empty_config() {
        let agent_identity = test_agent_identity();

        let hash = Hash::new("some-hash".into());
        let config_map = ConfigurationMap::new(
            serde_json::from_str::<HashMap<String, String>>(r#"{"config": ""}"#).unwrap(),
        );
        let remote_config = RemoteConfig::new(agent_identity.id.clone(), hash, Some(config_map));

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &remote_config, Ok(()));

        let handler = AgentRemoteConfigHandler::new(vec![validator]);

        let result = handler.handle(agent_identity.clone(), &remote_config);

        assert!(result.unwrap().is_none());
    }
}
