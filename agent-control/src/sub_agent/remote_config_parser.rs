//! Parsing and validation of OpAMP remote configurations into a [RemoteConfig].

use crate::agent_type::variable::VariableDefinition;
use crate::agent_type::variable::variable_type::VariableTypeDefinition;
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
    #[error("could not load the Agent Type '{agent_type_id}': {err}")]
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
///   These are applied last and therefore take precedence over both the base and the blob-level override configs.
///   The `agent_identity`'s agent type is resolved through `agent_type_registry` (only when at
///   least one such override is present) to look up each path's declared variable type: `string`-typed
///   variables store the override text as-is, every other declared type still requires it to be
///   valid YAML. A path that doesn't match any declared variable is ignored with a `warn!` log,
///   matching how an unrecognized key is already handled when filling an agent type's variables.
/// - Returns `None` if the final merged configuration is empty.
///
/// # Example
///
/// Three `AGENT_CONFIG_PREFIX` entries are merged, `key2` is then replaced by the blob-level
/// override, and `key3` is replaced last by the per-variable override:
///
/// ```
/// # use newrelic_agent_control::agent_control::agent_id::AgentID;
/// # use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
/// # use newrelic_agent_control::agent_type::definition::AgentTypeDefinition;
/// # use newrelic_agent_control::agent_type::protocol_version::SUPPORTED_PROTOCOL_VERSION;
/// # use newrelic_agent_control::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
/// # use newrelic_agent_control::opamp::remote_config::hash::{ConfigState, Hash};
/// # use newrelic_agent_control::opamp::remote_config::{ConfigurationMap, OpampRemoteConfig};
/// # use newrelic_agent_control::sub_agent::identity::AgentIdentity;
/// # use newrelic_agent_control::sub_agent::remote_config_parser::extract_remote_config_values;
/// # use std::collections::HashMap;
/// #
/// # // A registry that always resolves `key3` as a `yaml`-typed variable.
/// # struct DocRegistry(AgentTypeDefinition);
/// # impl AgentTypeRegistry for DocRegistry {
/// #     fn get(&self, _: &AgentTypeID) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
/// #         Ok(self.0.clone())
/// #     }
/// # }
/// # let agent_type_yaml = format!(
/// #     r#"namespace: newrelic
/// # name: testagent
/// # version: 0.1.0
/// # platform: host
/// # operating_system: linux
/// # protocol_version: "{SUPPORTED_PROTOCOL_VERSION}"
/// # variables:
/// #   key3:
/// #     description: "d"
/// #     type: yaml
/// #     required: false
/// # deployment: {{}}
/// # "#
/// # );
/// # let registry = DocRegistry(AgentTypeDefinition::from_slice(agent_type_yaml.as_bytes()).unwrap());
/// # let agent_identity = AgentIdentity::from((
/// #     AgentID::try_from("my-agent").unwrap(),
/// #     AgentTypeID::try_from("newrelic/testagent:0.1.0").unwrap(),
/// # ));
/// #
/// let config_map = serde_json::from_value::<HashMap<String, String>>(serde_json::json!(
/// {
///     "agentConfig-1": "key1: value1",
///     "agentConfig-2": "key2: value2",
///     "agentConfig-3": "key3: value3",
///     "override.agentConfig": "key2: overridden2",
///     "overrideVariable.agentConfig.key3": "overridden3",
/// }
/// )).unwrap();
/// let opamp_remote_config = OpampRemoteConfig::new(
///     AgentID::try_from("my-agent").unwrap(),
///     Hash::from("some-hash"),
///     ConfigState::Applying,
///     ConfigurationMap::new(config_map),
/// );
///
/// let remote_config =
///     extract_remote_config_values(&opamp_remote_config, &agent_identity, &registry)
///         .unwrap()
///         .unwrap();
/// assert_eq!(
///     remote_config.config,
///     serde_json::from_value(serde_json::json!({
///         "key1": "value1",
///         "key2": "overridden2",
///         "key3": "overridden3",
///     }))
///     .unwrap()
/// );
/// ```
///
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

    let maybe_override_config = opamp_remote_config.agent_config_override().map_err(|err| {
        RemoteConfigParserError::InvalidValues(format!("getting override values: {err}"))
    })?;
    if let Some(override_content) = maybe_override_config {
        let override_config = YAMLConfig::try_from(override_content.as_str()).map_err(|err| {
            RemoteConfigParserError::InvalidValues(format!("decoding override values: {err}"))
        })?;
        config = YAMLConfig::merge_override(config, override_config);
    }

    let mut override_variables = opamp_remote_config
        .agent_config_override_variables_iter()
        .peekable();
    if override_variables.peek().is_some() {
        let variable_definitions =
            load_variable_definitions(agent_type_registry, &agent_identity.agent_type_id)?;

        for (variable_path, raw_value) in override_variables {
            if let Some(value) =
                parse_override_value(variable_path, raw_value, &variable_definitions)?
            {
                config
                    .override_variable_value(variable_path, value)
                    .map_err(|err| {
                        RemoteConfigParserError::InvalidValues(format!(
                            "overriding variable '{variable_path}': {err}"
                        ))
                    })?;
            } else {
                warn!(
                    variable = variable_path,
                    agent_type = agent_identity.agent_type_id.to_string(),
                    "Ignoring override for variable not declared in the Agent Type"
                );
            }
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

/// Parses the provided value to override considering the variable type defined in the corresponding definitions.
/// It returns `None` if the variable is not defined and a parsing error if cannot be parsed.
fn parse_override_value(
    variable_path: &str,
    value: &String,
    definitions: &HashMap<String, VariableDefinition>,
) -> Result<Option<serde_json::Value>, RemoteConfigParserError> {
    definitions
        .get(variable_path)
        .map(|definition| match definition.kind() {
            // Using explicit parsing for each type instead of `matches!` in case a new type is added.
            // Strings don't need yaml parsing.
            VariableTypeDefinition::String(_) => Ok(serde_json::Value::String(value.to_string())),
            // Other types needsto be a valid yaml
            VariableTypeDefinition::Bool(_)
            | VariableTypeDefinition::Number(_)
            | VariableTypeDefinition::StringMap(_)
            | VariableTypeDefinition::Yaml(_) => parse_yaml_value(value).map_err(|err| {
                RemoteConfigParserError::InvalidValues(format!(
                    "could not decode the override value for variable '{variable_path}': {err}"
                ))
            }),
        })
        .transpose()
}

/// Parses a raw YAML fragment into a [serde_json::Value], unlike [YAMLConfig] the root is not
/// required to be a mapping.
///
/// Used to parse the value of a per-variable override, which can be a scalar, list, or mapping.
fn parse_yaml_value(value: &str) -> Result<serde_json::Value, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(
        value,
        serde_saphyr::options! {
            duplicate_keys: serde_saphyr::DuplicateKeyPolicy::LastWins,
        },
    )
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::{
        AgentRemoteConfigParser, RemoteConfigParser, RemoteConfigParserError, parse_yaml_value,
    };
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

    #[rstest]
    #[case::string("plain string", json!("plain string"))]
    #[case::mapping("key: value", json!({"key": "value"}))]
    #[case::list("[1, 2, 3]", json!([1, 2, 3]))]
    #[case::null("null", serde_json::Value::Null)]
    fn test_parse_yaml_value(#[case] input: &str, #[case] expected: serde_json::Value) {
        assert_eq!(parse_yaml_value(input).unwrap(), expected);
    }

    #[test]
    fn test_parse_yaml_value_invalid_yaml_errors() {
        assert!(parse_yaml_value("[unterminated").is_err());
    }
}
