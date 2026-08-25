//! Parsing and validation of OpAMP remote configurations into a [RemoteConfig].

use crate::agent_type::variable::VariableDefinition;
use crate::agent_type::{agent_type_id::AgentTypeID, registry::AgentTypeRegistry};
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::config::RemoteConfig;
use crate::values::yaml_config::YAMLConfig;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, warn};

type ErrorMessage = String;

/// Errors produced while parsing or validating a remote configuration.
#[derive(Debug, Error, Clone)]
pub enum RemoteConfigParserError {
    /// A configured validator rejected the configuration.
    #[error("remote configuration with validation errors: {0}")]
    Validation(ErrorMessage),
    /// The configuration arrived already marked as failed and cannot be loaded.
    #[error("remote configuration cannot be loaded: {0}")]
    RemoteConfigLoad(String),
    /// The configuration values are malformed (invalid YAML, duplicate keys, etc.).
    #[error("remote configuration with invalid values: {0}")]
    InvalidValues(String),
    /// Could not load the Agent Type
    #[error("could not load the agent type '{agent_type_id}': {err}")]
    AgentTypeLoad { agent_type_id: String, err: String },
}

/// Defines how to parse the OpAMP remote configuration in order to validate it and extract
/// the RemoteConfig with the corresponding values as [YAMLConfig] and Hash with status.
pub trait RemoteConfigParser {
    /// Parses and validates the remote configuration, returning the resulting [RemoteConfig],
    /// `None` when the configuration is empty (reset-to-local), or an error if it is invalid.
    fn parse(
        &self,
        agent_identity: AgentIdentity,
        config: &OpampRemoteConfig,
    ) -> Result<Option<RemoteConfig>, RemoteConfigParserError>;
}

/// A [RemoteConfigParser] that runs a sequence of [RemoteConfigValidator]s before extracting values.
pub struct AgentRemoteConfigParser<V, R> {
    remote_config_validators: Vec<V>,
    agent_type_registry: Arc<R>,
}

impl<V, R> AgentRemoteConfigParser<V, R>
where
    V: RemoteConfigValidator,
    R: AgentTypeRegistry,
{
    /// Creates a parser from the given list of remote-config validators and the agent type registry
    /// used to resolve the declared type of per-variable overrides.
    pub fn new(remote_config_validators: Vec<V>, agent_type_registry: Arc<R>) -> Self {
        AgentRemoteConfigParser {
            remote_config_validators,
            agent_type_registry,
        }
    }
}

impl<V, R> RemoteConfigParser for AgentRemoteConfigParser<V, R>
where
    V: RemoteConfigValidator,
    R: AgentTypeRegistry,
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
        extract_remote_config_values(config, &agent_identity, self.agent_type_registry.as_ref())
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
/// - Applies any per-variable overrides identified by
///   [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX),
///   setting the value at the dot-separated variable path (creating missing intermediate mappings as needed).
///   These are applied after the base and blob-level override configs, and therefore take precedence over
///   both. The `agent_identity`'s agent type is resolved through `agent_type_registry` (only when at
///   least one such override is present) to look up each path's declared variable type: `string`-typed
///   variables store the override text as-is, every other declared type still requires it to be
///   valid YAML. A path that doesn't match any declared variable is ignored with a `warn!` log,
///   matching how an unrecognized key is already handled when filling an agent type's variables.
/// - Applies any map-entry overrides, identified by a
///   [AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR](crate::opamp::remote_config::AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR)-delimited
///   suffix on the variable path, by merging the parsed value into the mapping living at that
///   path. These are applied last, after all whole-variable overrides, so a whole-variable
///   override and one or more map-entry overrides for the same path in the same payload compose
///   rather than conflict. Requires the target variable to be declared `string_map`.
/// - Returns `None` if the final merged configuration is empty.
///
/// # Example
///
/// Three `AGENT_CONFIG_PREFIX` entries are merged, `key2` is then replaced by the blob-level
/// override, and `key3` is replaced last by the per-variable override:
///
/// **Input**:
/// ```json
/// {
///     "agentConfig-1": "key1: value1",
///     "agentConfig-2": "key2: value2",
///     "agentConfig-3": "key3: value3",
///     "override.agentConfig": "key2: overridden2",
///     "variable.agentConfig.key3": "overridden3"
/// }
/// ```
/// **Output:**
/// ```json
/// {
///     "key1": "value1",
///     "key2": "overridden2",
///     "key3": "overridden3",
/// }
/// ```
/// # Errors
///
/// Returns [RemoteConfigParserError] if:
/// - Any configuration entry contains invalid YAML, including the override configs.
/// - Duplicate keys are found when merging configurations.
/// - There is more than one configuration starting with
///   [AGENT_CONFIG_OVERRIDE_PREFIX](crate::opamp::remote_config::AGENT_CONFIG_OVERRIDE_PREFIX)
/// - A per-variable override path is empty, or an intermediate segment of its path already holds a
///   non-mapping value.
/// - The agent's type cannot be resolved from `agent_type_registry` while at least one per-variable
///   override is present.
/// - A map-entry override (`:<map-key>` suffix) targets a variable that is not declared as
///   `string_map`, or uses an empty map key.
pub fn extract_remote_config_values<R: AgentTypeRegistry>(
    opamp_remote_config: &OpampRemoteConfig,
    agent_identity: &AgentIdentity,
    agent_type_registry: &R,
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

    let overrides = opamp_remote_config.overrides().map_err(|err| {
        RemoteConfigParserError::InvalidValues(format!("getting override values: {err}"))
    })?;

    if let Some(override_content) = overrides.blob() {
        let override_config = YAMLConfig::try_from(override_content.as_str()).map_err(|err| {
            RemoteConfigParserError::InvalidValues(format!("decoding override values: {err}"))
        })?;
        config = YAMLConfig::merge_override(config, override_config);
    }

    if overrides.has_variable_overrides() {
        let variable_definitions =
            load_variable_definitions(agent_type_registry, &agent_identity.agent_type_id)?;

        for variable_override in overrides.variables() {
            let Some(value) = variable_override
                .parse(&variable_definitions)
                .map_err(RemoteConfigParserError::InvalidValues)?
            else {
                // Ignored to support forward compatibility
                warn!(
                    variable = variable_override.path(),
                    agent_type = agent_identity.agent_type_id.to_string(),
                    "Ignoring override for variable not declared in the Agent Type"
                );
                continue;
            };
            config
                .override_variable_value(variable_override.path(), value)
                .map_err(|err| {
                    RemoteConfigParserError::InvalidValues(format!(
                        "overriding variable '{}': {err}",
                        variable_override.path()
                    ))
                })?;
        }

        for map_entry_override in overrides.map_entries() {
            let Some(value) = map_entry_override
                .parse(&variable_definitions)
                .map_err(RemoteConfigParserError::InvalidValues)?
            else {
                // Ignored to support forward compatibility.
                warn!(
                    variable = map_entry_override.path(),
                    map_key = map_entry_override.map_key(),
                    agent_type = agent_identity.agent_type_id.to_string(),
                    "Ignoring override for variable not declared in the Agent Type"
                );
                continue;
            };
            config
                .override_variable_map_entry(
                    map_entry_override.path(),
                    map_entry_override.map_key(),
                    value,
                )
                .map_err(|err| {
                    RemoteConfigParserError::InvalidValues(format!(
                        "overriding variable '{}:{}': {err}",
                        map_entry_override.path(),
                        map_entry_override.map_key()
                    ))
                })?;
        }
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

/// Obtains the map of [VariableDefinition] corresponding to the provided [AgentTypeID] defined in the registry.
fn load_variable_definitions<R: AgentTypeRegistry>(
    agent_type_registry: &R,
    agent_type_id: &AgentTypeID,
) -> Result<HashMap<String, VariableDefinition>, RemoteConfigParserError> {
    let agent_type = agent_type_registry.get(agent_type_id).map_err(|err| {
        RemoteConfigParserError::AgentTypeLoad {
            agent_type_id: agent_type_id.to_string(),
            err: err.to_string(),
        }
    })?;
    Ok(agent_type.variables.flatten())
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::{AgentRemoteConfigParser, RemoteConfigParser, RemoteConfigParserError};
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::protocol_version::SUPPORTED_PROTOCOL_VERSION;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidator;
    use crate::opamp::remote_config::{
        AGENT_CONFIG_OVERRIDE_PREFIX, AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX, AGENT_CONFIG_PREFIX,
        ConfigurationMap, OpampRemoteConfig,
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

    /// A `string`-typed variable declaration snippet, for use with [agent_type_definition_with_variable].
    const STRING_VAR: &str = "description: \"d\"\ntype: string\nrequired: true";
    /// A `yaml`-typed variable declaration snippet, for use with [agent_type_definition_with_variable].
    const YAML_VAR: &str = "description: \"d\"\ntype: yaml\nrequired: false";
    /// A `string_map`-typed variable declaration snippet, for use with [agent_type_definition_with_variable].
    const STRING_MAP_VAR: &str =
        "description: \"d\"\ntype: string_map\nrequired: false\ndefault: {}";

    /// Builds an [AgentTypeDefinition] declaring a single variable at `path` (dot-separated,
    /// creating intermediate mappings as needed) with the given type declaration snippet
    /// (one of [STRING_VAR]/[YAML_VAR]).
    fn agent_type_definition_with_variable(path: &str, type_snippet: &str) -> AgentTypeDefinition {
        let segments: Vec<&str> = path.split('.').collect();
        let mut variables_yaml = String::new();
        for (level, segment) in segments.iter().enumerate() {
            variables_yaml.push_str(&"  ".repeat(level + 1));
            variables_yaml.push_str(segment);
            variables_yaml.push_str(":\n");
        }
        let leaf_indent = "  ".repeat(segments.len() + 1);
        for line in type_snippet.lines() {
            variables_yaml.push_str(&leaf_indent);
            variables_yaml.push_str(line);
            variables_yaml.push('\n');
        }

        let yaml = format!(
            "namespace: newrelic\nname: testagent\nversion: 0.1.0\nplatform: host\noperating_system: linux\nprotocol_version: \"{SUPPORTED_PROTOCOL_VERSION}\"\nvariables:\n{variables_yaml}deployment: {{}}\n"
        );
        AgentTypeDefinition::from_slice(yaml.as_bytes()).unwrap()
    }

    /// Builds a registry mock for `config`: if `config` carries no
    /// `AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX` key, the registry must never be queried. Otherwise it
    /// resolves the agent identity's type to an [AgentTypeDefinition] declaring `declared_variable`
    /// (or no variables at all, when `None`, to exercise the "unknown path" case).
    fn registry_for(
        agent_identity: &AgentIdentity,
        config: &serde_json::Value,
        declared_variable: Option<(&str, &str)>,
    ) -> MockAgentTypeRegistry {
        let mut registry = MockAgentTypeRegistry::new();
        let has_override_variable_keys = config.as_object().is_some_and(|obj| {
            obj.keys()
                .any(|k| k.starts_with(AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX))
        });

        if has_override_variable_keys {
            let definition = match declared_variable {
                Some((path, type_snippet)) => {
                    agent_type_definition_with_variable(path, type_snippet)
                }
                None => {
                    AgentTypeDefinition::empty_with_metadata(agent_identity.agent_type_id.clone())
                }
            };
            registry.should_get(agent_identity.agent_type_id.clone(), &definition);
        } else {
            registry.expect_get().never();
        }
        registry
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

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get().never();

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator, _>::new(
            Vec::new(),
            Arc::new(registry),
        );
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

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get().never();

        let handler = AgentRemoteConfigParser::new(
            vec![validator1, validator2, validator3],
            Arc::new(registry),
        );

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::Validation(s)) => {
            assert_eq!(s, "validation2 error".to_string());
        });
    }

    #[rstest]
    #[case::invalid_yaml_config_single_value(
        json!({AGENT_CONFIG_PREFIX: "single-value"}),
        None
    )]
    #[case::invalid_yaml_config_array(
        json!({AGENT_CONFIG_PREFIX: "[1, 2, 3]"}),
        None
    )]
    #[case::mutiple_configs_duplicated_keys(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "key: value2"}),
        None
    )]
    #[case::mutiple_configs_config_single_value(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "single-value"}),
        None
    )]
    #[case::mutiple_configs_config_array(
        json!({format!("{AGENT_CONFIG_PREFIX}-1"): "key: value", format!("{AGENT_CONFIG_PREFIX}-2"): "[1, 2, 3]"}),
        None
    )]
    #[case::invalid_override_yaml_single_value(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "single-value"}),
        None
    )]
    #[case::invalid_override_yaml_array(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "[1, 2, 3]"}),
        None
    )]
    #[case::multiple_override_configs(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: "key: value2", format!("{AGENT_CONFIG_OVERRIDE_PREFIX}-2"): "key: value3"}),
        None
    )]
    #[case::invalid_override_variable_yaml(
        json!({AGENT_CONFIG_PREFIX: "key: value", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key"): "[unterminated"}),
        Some(("key", YAML_VAR))
    )]
    #[case::override_variable_type_conflict(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1.nested"): "value2"}),
        Some(("key1.nested", YAML_VAR))
    )]
    #[case::override_variable_yaml_type_rejects_invalid_text(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1"): "]"}),
        Some(("key1", YAML_VAR))
    )]
    #[case::override_variable_map_entry_wrong_type(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1:file1.yaml"): "content: v1"}),
        Some(("key1", YAML_VAR))
    )]
    #[case::override_variable_map_entry_conflicts_with_non_mapping(
        json!({AGENT_CONFIG_PREFIX: "key1: not-a-map", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1:file1.yaml"): "content: v1"}),
        Some(("key1", STRING_MAP_VAR))
    )]
    fn test_invalid_agent_configs_remote_values(
        #[case] config: serde_json::Value,
        #[case] agent_type_declared_variable: Option<(&str, &str)>,
    ) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let registry = registry_for(&agent_identity, &config, agent_type_declared_variable);
        let config_map = ConfigurationMap::new(
            serde_json::from_value::<HashMap<String, String>>(config).unwrap(),
        );
        let remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator, _>::new(
            Vec::new(),
            Arc::new(registry),
        );

        let result = handler.parse(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::InvalidValues(_)));
    }

    #[test]
    fn test_override_variable_map_entry_empty_key_errors_before_registry_lookup() {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(HashMap::from([
            (AGENT_CONFIG_PREFIX.to_string(), "key1: value1".to_string()),
            (
                format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1:"),
                "content: v1".to_string(),
            ),
        ]));
        let remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get().never();

        let handler = AgentRemoteConfigParser::<MockRemoteConfigValidator, _>::new(
            Vec::new(),
            Arc::new(registry),
        );

        let result = handler.parse(agent_identity.clone(), &remote_config);
        assert_matches!(result, Err(RemoteConfigParserError::InvalidValues(s)) => {
            assert!(s.contains("empty map-entry key for override variable 'key1'"));
        });
    }

    #[rstest]
    #[case::single_agent_config(
        json!({AGENT_CONFIG_PREFIX: "key: value"}),
        None,
        "key: value"
    )]
    #[case::multiple_agent_configs(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-2"): "key2: value2"}),
        None,
        "key1: value1\nkey2: value2"
    )]
    #[case::multiple_agent_configs_empty_config(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-empty"): ""}),
        None,
        "key1: value1"
    )]
    #[case::multiple_config(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", "non-agent-config": "key2: value2"}),
        None,
        "key1: value1"
    )]
    #[case::override_single_key(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: overridden"}),
        None,
        "key1: value1\nkey2: overridden"
    )]
    #[case::override_adds_new_key(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: value2"}),
        None,
        "key1: value1\nkey2: value2"
    )]
    #[case::override_multiple_keys(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2\nkey3: value3", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: overridden2\nkey3: overridden3"}),
        None,
        "key1: value1\nkey2: overridden2\nkey3: overridden3"
    )]
    #[case::override_with_multiple_agent_configs(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_PREFIX}-2"): "key2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key1: overridden"}),
        None,
        "key1: overridden\nkey2: value2"
    )]
    #[case::override_with_suffix(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", format!("{AGENT_CONFIG_OVERRIDE_PREFIX}-1"): "key2: overridden"}),
        None,
        "key1: value1\nkey2: overridden"
    )]
    #[case::override_empty(
        json!({AGENT_CONFIG_PREFIX: "key: value", AGENT_CONFIG_OVERRIDE_PREFIX: ""}),
        None,
        "key: value"
    )]
    #[case::override_only(
        json!({AGENT_CONFIG_OVERRIDE_PREFIX: "key1: overridden"}),
        None,
        "key1: overridden"
    )]
    #[case::override_null_does_not_remove_key_keeps_null(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2: null"}),
        None,
        "key1: value1\nkey2: null"
    )]
    #[case::override_empty_does_not_remove_key_keeps_empty(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", AGENT_CONFIG_OVERRIDE_PREFIX: "key2:\n"}),
        None,
        "key1: value1\nkey2:\n"
    )]
    #[case::inner_values_are_not_merged(
        json!({AGENT_CONFIG_PREFIX: r#"key1: {"key1_1": "value_1_1"}"#, AGENT_CONFIG_OVERRIDE_PREFIX: r#"key1: {"overridden_key": "overridden_value"}"#}),
        None,
        r#"key1: {"overridden_key": "overridden_value"}"#
    )]
    #[case::override_variable_top_level_key(
        json!({AGENT_CONFIG_PREFIX: "key1: value1\nkey2: value2", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key2"): "overridden2"}),
        Some(("key2", YAML_VAR)),
        "key1: value1\nkey2: overridden2"
    )]
    #[case::override_variable_nested_path(
        json!({AGENT_CONFIG_PREFIX: "foo:\n  bar: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.foo.bar"): "overridden"}),
        Some(("foo.bar", YAML_VAR)),
        "foo:\n  bar: overridden"
    )]
    #[case::override_variable_creates_missing_path(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.foo.bar"): "value"}),
        Some(("foo.bar", YAML_VAR)),
        "key1: value1\nfoo:\n  bar: value"
    )]
    #[case::override_variable_applies_after_blob_override(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", AGENT_CONFIG_OVERRIDE_PREFIX: "key1: blob_override", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1"): "variable_override"}),
        Some(("key1", YAML_VAR)),
        "key1: variable_override"
    )]
    #[case::override_variable_nested_path_applies_after_blob_override(
        json!({AGENT_CONFIG_PREFIX: "foo:\n  bar: value1", AGENT_CONFIG_OVERRIDE_PREFIX: "foo:\n  bar: blob_override", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.foo.bar"): "variable_override"}),
        Some(("foo.bar", YAML_VAR)),
        "foo:\n  bar: variable_override"
    )]
    #[case::override_variable_non_scalar_value(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key2"): "- a\n- b"}),
        Some(("key2", YAML_VAR)),
        "key1: value1\nkey2:\n  - a\n  - b"
    )]
    #[case::override_variable_empty_path_is_ignored(
        json!({AGENT_CONFIG_PREFIX: "key: value", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}."): "value"}),
        None,
        "key: value"
    )]
    #[case::override_variable_unknown_path_is_ignored(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.unknown"): "ignored"}),
        None,
        "key1: value1"
    )]
    #[case::override_variable_string_type_accepts_raw_text(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1"): "]"}),
        Some(("key1", STRING_VAR)),
        "key1: \"]\""
    )]
    #[case::override_variable_map_entry_accepts_raw_text(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file1.foo"): "]"}),
        Some(("string_map_var", STRING_MAP_VAR)),
        "key1: value1\nstring_map_var:\n  file1.foo: \"]\""
    )]
    #[case::override_variable_map_entry_invalid_yaml_falls_back_to_raw_text(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file1.yaml"): "[unterminated"}),
        Some(("string_map_var", STRING_MAP_VAR)),
        "key1: value1\nstring_map_var:\n  file1.yaml: \"[unterminated\""
    )]
    #[case::override_variable_map_entry_creates_map(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file1.yaml"): "content: whatever-1"}),
        Some(("string_map_var", STRING_MAP_VAR)),
        "key1: value1\nstring_map_var:\n  file1.yaml:\n    content: whatever-1"
    )]
    #[case::override_variable_map_entry_multiple_files_merge(
        json!({
            AGENT_CONFIG_PREFIX: "key1: value1",
            format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file1.yaml"): "content: whatever-1",
            format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file2.yaml"): "content: whatever-2"
        }),
        Some(("string_map_var", STRING_MAP_VAR)),
        "key1: value1\nstring_map_var:\n  file1.yaml:\n    content: whatever-1\n  file2.yaml:\n    content: whatever-2"
    )]
    #[case::override_variable_whole_then_map_entry_merges(
        json!({
            AGENT_CONFIG_PREFIX: "key1: value1",
            format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var"): "file1.yaml:\n  content: old\nfile3.yaml:\n  content: keep",
            format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.string_map_var:file1.yaml"): "content: new"
        }),
        Some(("string_map_var", STRING_MAP_VAR)),
        "key1: value1\nstring_map_var:\n  file1.yaml:\n    content: new\n  file3.yaml:\n    content: keep"
    )]
    #[case::override_variable_map_entry_unknown_path_is_ignored(
        json!({AGENT_CONFIG_PREFIX: "key1: value1", format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.unknown:file1.yaml"): "content: v1"}),
        None,
        "key1: value1"
    )]
    fn test_valid_remote_config_values(
        #[case] config: serde_json::Value,
        #[case] agent_type_declared_variable: Option<(&str, &str)>,
        #[case] expected_yaml: &str,
    ) {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let registry = registry_for(&agent_identity, &config, agent_type_declared_variable);
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

        let handler = AgentRemoteConfigParser::new(vec![validator], Arc::new(registry));

        let expected = RemoteConfig {
            config: serde_saphyr::from_str(expected_yaml).unwrap(),
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

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get().never();

        let handler = AgentRemoteConfigParser::new(vec![validator], Arc::new(registry));

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);

        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_override_variable_agent_type_not_found_in_registry_errors() {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(HashMap::from([
            (AGENT_CONFIG_PREFIX.to_string(), "key1: value1".to_string()),
            (
                format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1"),
                "value2".to_string(),
            ),
        ]));
        let opamp_remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &opamp_remote_config, Ok(()));

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get_not_found(agent_identity.agent_type_id.clone());

        let handler = AgentRemoteConfigParser::new(vec![validator], Arc::new(registry));

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);
        assert_matches!(
            result,
            Err(RemoteConfigParserError::AgentTypeLoad {
                agent_type_id,
                err: _
            }) => {
                assert_eq!(agent_type_id, agent_identity.agent_type_id.to_string())
            }
        );
    }

    #[test]
    fn test_override_error_getting_agent_type_from_registry_errors() {
        let agent_identity = AgentIdentity::default();

        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(HashMap::from([
            (AGENT_CONFIG_PREFIX.to_string(), "key1: value1".to_string()),
            (
                format!("{AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX}.key1"),
                "value2".to_string(),
            ),
        ]));
        let opamp_remote_config =
            OpampRemoteConfig::new(agent_identity.id.clone(), hash, state, config_map);

        let mut validator = MockRemoteConfigValidator::new();
        validator.should_validate(&agent_identity, &opamp_remote_config, Ok(()));

        let mut registry = MockAgentTypeRegistry::new();
        registry.expect_get_remote_error(agent_identity.agent_type_id.clone());

        let handler = AgentRemoteConfigParser::new(vec![validator], Arc::new(registry));

        let result = handler.parse(agent_identity.clone(), &opamp_remote_config);
        assert_matches!(
            result,
            Err(RemoteConfigParserError::AgentTypeLoad {
                agent_type_id,
                err: _
            }) => {
                assert_eq!(agent_type_id, agent_identity.agent_type_id.to_string())
            }
        );
    }
}
