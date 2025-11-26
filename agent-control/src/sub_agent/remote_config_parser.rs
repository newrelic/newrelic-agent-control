use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::config::RemoteConfig;
use crate::values::yaml_config::YAMLConfig;
use thiserror::Error;
use tracing::{debug, error};

type ErrorMessage = String;

#[derive(Debug, Error, Clone)]
pub enum RemoteConfigParserError {
    #[error("remote configuration with validation errors: {0}")]
    Validation(ErrorMessage),
    #[error("remote configuration cannot be loaded: {0}")]
    RemoteConfigLoad(String),
    #[error("remote configuration with invalid values: {0}")]
    InvalidValues(String),
}

/// Defines how to parse the OpAMP remote configuration in order to validate it and extract
/// the RemoteConfig with the corresponding values as [YAMLConfig] and Hash with status.
pub trait RemoteConfigParser {
    fn parse(
        &self,
        agent_identity: AgentIdentity,
        config: &OpampRemoteConfig,
    ) -> Result<Option<RemoteConfig>, RemoteConfigParserError>;
}

pub struct AgentRemoteConfigParser<V> {
    remote_config_validators: Vec<V>,
}

impl<V> AgentRemoteConfigParser<V>
where
    V: RemoteConfigValidator,
{
    pub fn new(remote_config_validators: Vec<V>) -> Self {
        AgentRemoteConfigParser {
            remote_config_validators,
        }
    }
}

impl<V> RemoteConfigParser for AgentRemoteConfigParser<V>
where
    V: RemoteConfigValidator,
{
    /// Handles the remote configuration received by the OpAMP client and returns the corresponding yaml configuration
    /// or an error if the configuration is invalid according to the configured validators.
    fn parse(
        &self,
        agent_identity: AgentIdentity,
        config: &OpampRemoteConfig,
    ) -> Result<Option<RemoteConfig>, RemoteConfigParserError> {
        // Errors here will cause the sub-agent to continue running with the previous configuration.
        // The supervisor won't be recreated.
        if let Some(err_msg) = config.state.error_message().cloned() {
            return Err(RemoteConfigParserError::RemoteConfigLoad(err_msg));
        }
        for validator in &self.remote_config_validators {
            if let Err(error_msg) = validator.validate(&agent_identity, config) {
                debug!(
                    hash = &config.hash.to_string(),
                    "Invalid remote configuration: {error_msg}"
                );
                return Err(RemoteConfigParserError::Validation(error_msg.to_string()));
            }
        }
        extract_remote_config_values(config)
    }
}

/// Extracts the opamp remote configuration values and parses them to [YAMLConfig], if the values are empty it returns None.
fn extract_remote_config_values(
    opamp_remote_config: &OpampRemoteConfig,
) -> Result<Option<RemoteConfig>, RemoteConfigParserError> {
    let config = opamp_remote_config.configs_iter().try_fold(
        YAMLConfig::default(),
        |mut acc, (_, content)| {
            let cfg = YAMLConfig::try_from(content.as_str()).map_err(|err| {
                RemoteConfigParserError::InvalidValues(format!("decoding config: {err}"))
            })?;
            acc.append(cfg).map_err(|err| {
                RemoteConfigParserError::InvalidValues(format!("appending config: {err}"))
            })?;
            Ok(acc)
        },
    )?;

    if config.is_empty() {
        return Ok(None);
    }

    Ok(Some(RemoteConfig {
        config,
        hash: opamp_remote_config.hash.clone(),
        state: opamp_remote_config.state.clone(),
    }))
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;

    use super::{AgentRemoteConfigParser, RemoteConfigParser, RemoteConfigParserError};
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidator;
    use crate::opamp::remote_config::{
        ConfigurationMap, DEFAULT_AGENT_CONFIG_IDENTIFIER, OpampRemoteConfig,
    };
    use crate::sub_agent::identity::AgentIdentity;
    use crate::values::config::RemoteConfig;
    use assert_matches::assert_matches;
    use mockall::mock;
    use predicates::prelude::predicate;
    use rstest::rstest;

    mock! {
        pub RemoteConfigParser {}

        impl RemoteConfigParser for RemoteConfigParser{
            fn parse(
                &self,
                agent_identity: AgentIdentity,
                config: &OpampRemoteConfig,
            ) -> Result<Option<RemoteConfig>, RemoteConfigParserError>;
        }
    }

    impl MockRemoteConfigParser {
        pub fn should_parse(
            &mut self,
            agent_identity: AgentIdentity,
            config: OpampRemoteConfig,
            remote_config: Option<RemoteConfig>,
        ) {
            self.expect_parse()
                .once()
                .with(predicate::eq(agent_identity), predicate::eq(config))
                .return_once(|_, _| Ok(remote_config));
        }
    }

    #[test]
    fn test_agent_remote_config_parser_config_with_previous_errors() {
        let agent_identity = AgentIdentity::default();
        // The hash had some previous errors
        let hash = Hash::from("some-hash");
        let state = ConfigState::Failed {
            error_message: "some error".to_string(),
        };
        let opamp_remote_config = OpampRemoteConfig::new(
            agent_identity.id.clone(),
            hash,
            state,
            ConfigurationMap::default(),
        );

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator>::new(Vec::new());
        let result = handler.parse(agent_identity, &opamp_remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::RemoteConfigLoad(s)) => {
            assert_eq!(s, "some error".to_string());
        });
    }

    #[test]
    fn test_agent_remote_config_parser_config_validation_error() {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let opamp_remote_config = OpampRemoteConfig::new(
            agent_identity.id.clone(),
            hash,
            state,
            ConfigurationMap::default(),
        );

        let mut validator1 = MockRemoteConfigValidator::new();
        let mut validator2 = MockRemoteConfigValidator::new();
        let mut validator3 = MockRemoteConfigValidator::new();

        validator1.should_validate(&agent_identity, &opamp_remote_config, Ok(()));
        validator2.should_validate(
            &agent_identity,
            &opamp_remote_config,
            Err("validation2 error".into()),
        );
        validator3.expect_validate().never();

        let handler = AgentRemoteConfigParser::new(vec![validator1, validator2, validator3]);

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::Validation(s)) => {
            assert_eq!(s, "validation2 error".to_string());
        });
    }

    #[rstest]
    #[case::invalid_yaml_config_single_value(r#"{"config": "single-value"}"#)]
    #[case::invalid_yaml_config_array(r#"{"config": "[1, 2, 3]"}"#)]
    #[case::mutiple_configs_duplicated_keys(
        r#"{"config1": "{\"key\": \"value\"}", "config2": "{\"key\": \"value2\"}"}"#
    )]
    #[case::mutiple_configs_config_single_value(
        r#"{"config1": "{\"key\": \"value\"}", "config2": "single-value"}"#
    )]
    #[case::mutiple_configs_config_array(
        r#"{"config1": "{\"key\": \"value\"}", "config2": "[1, 2, 3]"}"#
    )]
    fn test_agent_remote_config_parser_config_invalid_values(#[case] config: &str) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map =
            ConfigurationMap::new(serde_json::from_str::<HashMap<String, String>>(config).unwrap());
        let remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator>::new(Vec::new());

        let result = handler.parse(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::InvalidValues(_)));
    }

    #[rstest]
    #[case::single_config(r#"{"config": "key: value"}"#, "key: value")]
    #[case::multiple_configs(
        r#"{"config1": "key1: value1", "config2": "key2: value2"}"#,
        "key1: value1\nkey2: value2"
    )]
    #[case::multiple_configs_empty_config(
        r#"{"config1": "key1: value1", "empty": ""}"#,
        "key1: value1"
    )]
    fn test_agent_remote_config_parser_some_config(
        #[case] config: &str,
        #[case] expected_yaml: &str,
    ) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map =
            ConfigurationMap::new(serde_json::from_str::<HashMap<String, String>>(config).unwrap());
        let opamp_remote_config = OpampRemoteConfig::new(
            agent_identity.id.clone(),
            hash.clone(),
            state.clone(),
            config_map,
        );

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &opamp_remote_config, Ok(()));

        let handler = AgentRemoteConfigParser::new(vec![validator]);

        let expected = RemoteConfig {
            config: serde_yaml::from_str(expected_yaml).unwrap(),
            hash,
            state,
        };

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);
        assert_matches!(result, Ok(Some(yaml_config)) => {
            assert_eq!(yaml_config, expected);
        });
    }

    #[test]
    fn test_agent_remote_config_parser_empty_config() {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(HashMap::from([(
            DEFAULT_AGENT_CONFIG_IDENTIFIER.to_string(),
            String::new(),
        )]));
        let opamp_remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &opamp_remote_config, Ok(()));

        let handler = AgentRemoteConfigParser::new(vec![validator]);

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);

        assert!(result.unwrap().is_none());
    }
}
