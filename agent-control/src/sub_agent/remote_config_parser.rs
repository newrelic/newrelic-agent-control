use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::config::RemoteConfig;
use crate::values::yaml_config::YAMLConfig;
use thiserror::Error;
use tracing::debug;

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

/// Extracts and merges OpAMP remote configuration values into a single [YAMLConfig].
///
/// This function:
/// - Processes all configuration entries that start with the
///   [AGENT_CONFIG_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_PREFIX) identifier.
///   Multiple configuration entries are merged into a single configuration, with key collisions
///   being treated as errors to ensure configuration integrity.
/// - Takes the configuration starting with
///   [AGENT_CONFIG_OVERRIDE_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_OVERRIDE_PREFIX) (if any) merges it
///   with the configuration taken from the
///   [AGENT_CONFIG_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_PREFIX) identifier.
///   The override configuration takes precedence, therefore key collisions are not errors in this case.
/// - Returns `None` if the final merged configuration is empty.
///
/// # Example
///
/// **Input**:
/// ```json
/// {
///   "<AGENT_CONFIG_PREFIX>-1": "key1: value1",
///   "<AGENT_CONFIG_PREFIX>-2": "key2: value2",
///   "<AGENT_CONFIG_PREFIX>-3": "key3: value3",
///   "<AGENT_CONFIG_OVERRIDE_PREFIX>": "key2: overridden"
/// }
/// ```
/// **Output:**
/// ```yaml
/// key1: value1
/// key2: overridden
/// key3: value3
/// ```
///
/// # Errors
///
/// Returns [RemoteConfigParserError] if:
/// - Any configuration entry contains invalid YAML, including the override config.
/// - Duplicate keys are found when merging configurations.
/// - There is more than one configuration starting with
///   [AGENT_CONFIG_OVERRIDE_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_OVERRIDE_PREFIX)
pub fn extract_remote_config_values(
    opamp_remote_config: &OpampRemoteConfig,
) -> Result<Option<RemoteConfig>, RemoteConfigParserError> {
    let mut config = opamp_remote_config.agent_configs_iter().try_fold(
        YAMLConfig::default(),
        |mut acc, (_, content)| {
            let cfg = YAMLConfig::try_from(content.as_str()).map_err(|err| {
                RemoteConfigParserError::InvalidValues(format!("decoding config: {err}"))
            })?;
            acc = YAMLConfig::try_append(acc, cfg).map_err(|err| {
                RemoteConfigParserError::InvalidValues(format!("appending config: {err}"))
            })?;
            Ok(acc)
        },
    )?;

    let maybe_override_config = opamp_remote_config.agent_config_override().map_err(|err| {
        RemoteConfigParserError::InvalidValues(format!("getting override values: {err}"))
    })?;
    if let Some(override_content) = maybe_override_config {
        let override_config = YAMLConfig::try_from(override_content.as_str()).map_err(|err| {
            RemoteConfigParserError::InvalidValues(format!("decoding override values: {err}"))
        })?;
        config = YAMLConfig::merge_override(config, override_config);
    }

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
        AGENT_CONFIG_OVERRIDE_PREFIX, AGENT_CONFIG_PREFIX, ConfigurationMap, OpampRemoteConfig,
    };
    use crate::sub_agent::identity::AgentIdentity;
    use crate::values::config::RemoteConfig;
    use assert_matches::assert_matches;
    use mockall::mock;
    use predicates::prelude::predicate;
    use rstest::rstest;
    use serde_json::json;

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
    #[case::invalid_yaml_config_single_value(
        json!({AGENT_CONFIG_PREFIX: "single-value"})
    )]
    #[case::invalid_yaml_config_array(
        json!({AGENT_CONFIG_PREFIX: "[1, 2, 3]"})
    )]
    #[case::mutiple_configs_duplicated_keys(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "key: value2"})
    )]
    #[case::mutiple_configs_config_single_value(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "single-value"})
    )]
    #[case::mutiple_configs_config_array(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "[1, 2, 3]"})
    )]
    #[case::invalid_override_yaml_single_value(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "single-value"})
    )]
    #[case::invalid_override_yaml_array(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "[1, 2, 3]"})
    )]
    #[case::multiple_override_configs(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "key: value2", format!("{AGENT_CONFIG_OVERRIDE_PREFIX}-2"): "key: value3"})
    )]
    fn test_invalid_agent_configs_remote_values(#[case] config: serde_json::Value) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(
            serde_json::from_value::<HashMap<String, String>>(config).unwrap(),
        );
        let remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator>::new(Vec::new());

        let result = handler.parse(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::InvalidValues(_)));
    }

    #[rstest]
    #[case::single_agent_config(
        json!({AGENT_CONFIG_PREFIX: "key: value"}),
        "key: value"
    )]
    #[case::multiple_agent_configs(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-2"): "key2: value2"}),
        "key1: value1\nkey2: value2"
    )]
    #[case::multiple_agent_configs_empty_config(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-empty"): ""}),
        "key1: value1"
    )]
    #[case::multiple_config(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", "non-agent-config": "key2: value2"}),
        "key1: value1"
    )]
    #[case::override_single_key(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: overridden"}),
        "key1: value1\nkey2: overridden"
    )]
    #[case::override_adds_new_key(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: value2"}),
        "key1: value1\nkey2: value2"
    )]
    #[case::override_multiple_keys(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2\nkey3: value3", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: overridden2\nkey3: overridden3"}),
        "key1: value1\nkey2: overridden2\nkey3: overridden3"
    )]
    #[case::override_with_multiple_agent_configs(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-2"): "key2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key1: overridden"}),
        "key1: overridden\nkey2: value2"
    )]
    #[case::override_with_suffix(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", format!("{AGENT_CONFIG_OVERRIDE_PREFIX}-1"): "key2: overridden"}),
        "key1: value1\nkey2: overridden"
    )]
    #[case::override_empty(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: ""}),
        "key: value"
    )]
    #[case::override_only(
        json!({AGENT_CONFIG_OVERRIDE_PREFIX: "key1: overridden"}),
        "key1: overridden"
    )]
    #[case::override_null_does_not_remove_key_keeps_null(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: null"}),
        "key1: value1\nkey2: null"
    )]
    #[case::override_empty_does_not_remove_key_keeps_empty(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2:\n"}),
        "key1: value1\nkey2:\n"
    )]
    #[case::inner_values_are_not_merged(
        json!({AGENT_CONFIG_PREFIX: r#"key1: {"key1_1": "value_1_1"}"#, AGENT_CONFIG_OVERRIDE_PREFIX: r#"key1: {"overridden_key": "overridden_value"}"#}),
        r#"key1: {"overridden_key": "overridden_value"}"#
    )]
    fn test_valid_remote_config_values(
        #[case] config: serde_json::Value,
        #[case] expected_yaml: &str,
    ) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(
            serde_json::from_value::<HashMap<String, String>>(config).unwrap(),
        );
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
            AGENT_CONFIG_PREFIX.to_string(),
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
