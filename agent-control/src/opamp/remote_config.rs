//! Remote configuration received via OpAMP: its model, configuration map, hashes, and validators.
use crate::agent_control::agent_id::AgentID;
use crate::agent_type::templates::TEMPLATE_KEY_SEPARATOR;
use crate::agent_type::variable::VariableDefinition;
use crate::agent_type::variable::variable_type::VariableTypeDefinition;
use crate::opamp::remote_config::hash::ConfigState;
use crate::opamp::remote_config::{hash::Hash, signature::SignatureData};
use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, EffectiveConfig};
use signature::Signatures;
use std::collections::HashMap;
use std::string::FromUtf8Error;
use thiserror::Error;
use tracing::warn;

pub mod hash;
pub mod report;
pub mod signature;
pub mod validators;

/// Prefix that identifies the agent configuration keys within the OpAMP [opamp_client::opamp::proto::AgentConfigMap].
/// Any key that starts with this prefix is considered part of the agent configuration. See parsing implementation
/// for each case.
pub const AGENT_CONFIG_PREFIX: &str = "agentConfig";

/// Prefix that identifies an agent configuration that should override the values considered part of the configuration.
/// See the parsing implementation at [extract_remote_config_values](crate::sub_agent::remote_config_parser::extract_remote_config_values)
/// for details.
pub const AGENT_CONFIG_OVERRIDE_PREFIX: &str = "override.agentConfig";

/// Prefix that identifies an override for a single, possibly nested, configuration variable. The variable path
/// follows the prefix separated by a dot, e.g. `variable.agentConfig.foo.bar`. See the parsing
/// implementation at [extract_remote_config_values](crate::sub_agent::remote_config_parser::extract_remote_config_values)
/// for details.
pub const AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX: &str = "variable.agentConfig";

/// Separator between a per-variable override's dot-separated path and an optional map-entry key,
/// e.g. `variable.agentConfig.config_integrations:file1.yaml`. Only meaningful for
/// `string_map`-typed variables; see the parsing implementation at
/// [extract_remote_config_values](crate::sub_agent::remote_config_parser::extract_remote_config_values)
/// for details.
pub const AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR: char = ':';

/// This structure represents the remote configuration that we would retrieve from a server via OpAMP.
/// Contains identifying metadata and the actual configuration values
#[derive(Debug, PartialEq, Clone)]
pub struct OpampRemoteConfig {
    /// Identifier of the agent this configuration targets.
    pub agent_id: AgentID,
    /// Hash identifying this configuration version.
    pub hash: Hash,
    /// Application state of this configuration.
    pub state: ConfigState,
    signatures: Option<Signatures>,
    config_map: ConfigurationMap,
}

/// Errors produced while parsing or inspecting a remote configuration.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum OpampRemoteConfigError {
    /// A configuration body was not valid UTF-8.
    #[error("invalid UTF-8 sequence: {0}")]
    UTF8(#[from] FromUtf8Error),

    /// The configuration was structurally invalid for the given hash.
    #[error("invalid config for hash '{0}': {1}")]
    InvalidConfig(String, String),
}

impl OpampRemoteConfig {
    /// Creates a remote config with the given agent, hash, state and configuration map.
    pub fn new(
        agent_id: AgentID,
        hash: Hash,
        state: ConfigState,
        config_map: ConfigurationMap,
    ) -> Self {
        Self {
            agent_id,
            hash,
            state,
            config_map,
            signatures: None,
        }
    }

    /// Add signature data to the remote config
    pub fn with_signature(self, signatures: Signatures) -> Self {
        Self {
            signatures: Some(signatures),
            ..self
        }
    }

    /// Returns an iterator over the configuration key-value pairs.
    pub fn configs_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.config_map.0.iter()
    }

    /// Returns an iterator over the configuration key-value pairs that start with [AGENT_CONFIG_PREFIX].
    pub fn agent_configs_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.configs_iter()
            .filter(|(k, _)| k.starts_with(AGENT_CONFIG_PREFIX))
    }

    /// Returns true if there are no agent configuration key-value pairs that start with [AGENT_CONFIG_PREFIX]
    /// or all such key-value pairs have empty values.
    pub fn is_agent_configs_empty(&self) -> bool {
        !self
            .config_map
            .0
            .iter()
            .any(|(k, v)| k.starts_with(AGENT_CONFIG_PREFIX) && !v.is_empty())
    }

    /// Classifies every override recognized in the config map into an [Overrides] view — see its
    /// fields for the recognized kinds. Fails if more than one blob-level override (identified by
    /// [AGENT_CONFIG_OVERRIDE_PREFIX]) is found, since only one is supported.
    pub fn overrides(&self) -> Result<Overrides<'_>, OpampRemoteConfigError> {
        let mut blob = None;
        let mut variables = Vec::new();
        let mut map_entries = Vec::new();

        // agentConfig keys are not overrides
        let configs = self
            .configs_iter()
            .filter(|(key, _)| !key.starts_with(AGENT_CONFIG_PREFIX));

        for (k, v) in configs {
            if k.starts_with(AGENT_CONFIG_OVERRIDE_PREFIX) {
                if blob.is_some() {
                    return Err(OpampRemoteConfigError::InvalidConfig(
                        self.hash.to_string(),
                        format!(
                            "multiple configurations with '{AGENT_CONFIG_OVERRIDE_PREFIX}' prefix"
                        ),
                    ));
                }
                blob = Some(v);
                continue;
            }
            match parse_override_variable_key(k) {
                None => warn!(key = k, "Config-map key not recognized"),
                Some((variable_path, None)) => {
                    variables.push(VariableOverride {
                        path: variable_path,
                        raw_value: v.as_str(),
                    });
                }
                Some((variable_path, Some(""))) => {
                    return Err(OpampRemoteConfigError::InvalidConfig(
                        self.hash.to_string(),
                        format!("empty map-entry key for override variable '{variable_path}'"),
                    ));
                }
                Some((variable_path, Some(map_key))) => {
                    map_entries.push(MapEntryOverride {
                        path: variable_path,
                        map_key,
                        raw_value: v.as_str(),
                    });
                }
            }
        }

        Ok(Overrides {
            blob,
            variables,
            map_entries,
        })
    }

    /// Get the signature data for a config key
    pub fn signature(&self, config_name: &str) -> Result<SignatureData, OpampRemoteConfigError> {
        let Some(signatures) = &self.signatures else {
            return Err(OpampRemoteConfigError::InvalidConfig(
                self.hash.to_string(),
                "missing signatures".to_string(),
            ));
        };

        signatures
            .signatures
            .get(config_name)
            .cloned()
            .ok_or_else(|| {
                OpampRemoteConfigError::InvalidConfig(
                    self.hash.to_string(),
                    format!("missing signature for config: {}", config_name),
                )
            })
    }
}

/// The overrides recognized in a remote config's map, as classified by
/// [OpampRemoteConfig::overrides].
#[derive(Debug)]
pub struct Overrides<'a> {
    blob: Option<&'a String>,
    variables: Vec<VariableOverride<'a>>,
    map_entries: Vec<MapEntryOverride<'a>>,
}

impl<'a> Overrides<'a> {
    /// The blob-level override identified by [AGENT_CONFIG_OVERRIDE_PREFIX], if present. It
    /// overrides the whole merged agent configuration.
    pub fn blob(&self) -> Option<&String> {
        self.blob
    }

    /// Whole-variable overrides identified by [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX] with no
    /// `:map-key` suffix, overriding the value at their path entirely.
    pub fn variables(&self) -> &[VariableOverride<'a>] {
        &self.variables
    }

    /// Map-entry overrides identified by [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX] with a
    /// [AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR]-delimited suffix, each overriding a
    /// single entry of the `string_map` variable living at their path.
    pub fn map_entries(&self) -> &[MapEntryOverride<'a>] {
        &self.map_entries
    }

    /// True if there is at least one whole-variable or map-entry override to apply.
    pub fn has_variable_overrides(&self) -> bool {
        !self.variables.is_empty() || !self.map_entries.is_empty()
    }
}

/// A whole-variable override identified by [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX] with no
/// `:map-key` suffix: the dot-separated variable path (with the prefix stripped) and its raw
/// override value.
#[derive(Debug)]
pub struct VariableOverride<'a> {
    path: &'a str,
    raw_value: &'a str,
}

impl<'a> VariableOverride<'a> {
    /// The dot-separated variable path this override targets.
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The raw override value, as received over OpAMP.
    pub fn raw_value(&self) -> &'a str {
        self.raw_value
    }

    /// Parses this override's raw value considering the variable type defined in `definitions`.
    /// Returns `None` if the variable is not defined, and an error message if it is defined but
    /// its raw value cannot be parsed according to its declared type.
    pub fn parse(
        &self,
        definitions: &HashMap<String, VariableDefinition>,
    ) -> Result<Option<serde_json::Value>, String> {
        definitions
            .get(self.path)
            .map(|definition| match definition.kind() {
                // Using explicit parsing for each type instead of `matches!` in case a new type is added.
                // Strings don't need yaml parsing.
                VariableTypeDefinition::String(_) => {
                    Ok(serde_json::Value::String(self.raw_value.to_string()))
                }
                // Other types need to be a valid yaml in order to honor the variable type. Eg: a 'yaml' variable
                // needs to be a valid yaml (deserialization must succeed).
                VariableTypeDefinition::Bool(_)
                | VariableTypeDefinition::Number(_)
                | VariableTypeDefinition::StringMap(_)
                | VariableTypeDefinition::Yaml(_) => serde_saphyr::from_str(self.raw_value)
                    .map_err(|err| {
                        format!(
                            "could not decode the override value for variable '{}': {}",
                            self.path,
                            err.render_with_formatter(&serde_saphyr::UserMessageFormatter)
                        )
                    }),
            })
            .transpose()
    }
}

/// A map-entry override identified by [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX] with a
/// [AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR]-delimited suffix: the dot-separated variable
/// path, the map-entry key (the text after the separator, verbatim), and the raw override value.
#[derive(Debug)]
pub struct MapEntryOverride<'a> {
    path: &'a str,
    map_key: &'a str,
    raw_value: &'a str,
}

impl<'a> MapEntryOverride<'a> {
    /// The dot-separated variable path this override targets.
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The map-entry key (the text after the [AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR]), verbatim.
    pub fn map_key(&self) -> &'a str {
        self.map_key
    }

    /// Parses this override's raw value, enforcing that the target variable is declared as
    /// `string_map`. Returns `None` if the variable is not defined (ignored, consistent with
    /// whole-variable overrides), and an error message if it is defined but not a `string_map`.
    pub fn parse(
        &self,
        definitions: &HashMap<String, VariableDefinition>,
    ) -> Result<Option<serde_json::Value>, String> {
        let Some(definition) = definitions.get(self.path) else {
            return Ok(None);
        };
        if !matches!(definition.kind(), VariableTypeDefinition::StringMap(_)) {
            return Err(format!(
                "overriding variable '{}:{}': the ':<map-key>' override syntax is only supported for 'string_map' variables",
                self.path, self.map_key
            ));
        }

        Ok(Some(serde_saphyr::from_str(self.raw_value).unwrap_or_else(
            |_| {
                // Covers the case where the file content is not a valid YAML.
                // For example the following config:
                // may_string_map_config:
                //   my_file.non_yaml: "["
                serde_json::Value::String(self.raw_value.to_string())
            },
        )))
    }

    /// The raw override value, as received over OpAMP.
    pub fn raw_value(&self) -> &'a str {
        self.raw_value
    }
}

/// Parses a single config-map key that starts with [AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX] into
/// its dot-separated variable path and, if present, its map-entry key (the text after the first
/// [AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR] in the remainder). Returns `None` if `key`
/// doesn't match the prefix at all.
fn parse_override_variable_key(key: &str) -> Option<(&str, Option<&str>)> {
    let path = key
        .strip_prefix(AGENT_CONFIG_OVERRIDE_VARIABLE_PREFIX)?
        .strip_prefix(TEMPLATE_KEY_SEPARATOR)?;
    Some(
        match path.split_once(AGENT_CONFIG_OVERRIDE_VARIABLE_MAP_KEY_SEPARATOR) {
            Some((variable_path, map_key)) => (variable_path, Some(map_key)),
            None => (path, None),
        },
    )
}

/// This structure represents the actual configuration values that are stored in the remote config.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigurationMap(HashMap<String, String>);

impl ConfigurationMap {
    /// Creates a configuration map from the given key-value pairs.
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }
}

impl TryFrom<AgentConfigMap> for ConfigurationMap {
    type Error = OpampRemoteConfigError;

    fn try_from(agent_config_map: AgentConfigMap) -> Result<Self, Self::Error> {
        agent_config_map.config_map.into_iter().try_fold(
            ConfigurationMap::default(),
            |mut result: ConfigurationMap, (key, value)| {
                let body = String::from_utf8(value.body)?;
                let _ = result.0.insert(key, body.to_string());
                Ok(result)
            },
        )
    }
}

impl From<ConfigurationMap> for EffectiveConfig {
    fn from(value: ConfigurationMap) -> Self {
        let config_map = value
            .0
            .into_iter()
            .map(|(k, v)| {
                let agent_config_file = AgentConfigFile {
                    body: v.as_bytes().to_vec(),
                    content_type: "text/yaml".to_string(),
                };
                (k, agent_config_file)
            })
            .collect();

        let config_map = AgentConfigMap { config_map }.into();

        Self { config_map }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;
    use serde_json::json;

    /// Helper to build a [OpampRemoteConfig] for testing.
    fn testing_agent_config(config_map: serde_json::Value) -> OpampRemoteConfig {
        let agent_id = AgentID::try_from("test-agent").unwrap();
        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(
            serde_json::from_value::<HashMap<String, String>>(config_map).unwrap(),
        );
        OpampRemoteConfig::new(agent_id, hash, state, config_map)
    }

    #[rstest]
    #[case::single_agent_config(
        json!({"agentConfig": "key: value"}),
        json!({"agentConfig": "key: value"})
    )]
    #[case::multiple_agent_configs(
        json!({"agentConfig": "key1: value1", "agentConfig2": "key2: value2"}),
        json!({"agentConfig": "key1: value1", "agentConfig2": "key2: value2"})
    )]
    #[case::mixed_configs_filters_non_agent(
        json!({"agentConfig": "key1: value1", "otherConfig": "key2: value2", "agentConfig3": "key3: value3"}),
        json!({"agentConfig": "key1: value1", "agentConfig3": "key3: value3"})
    )]
    #[case::no_agent_configs(
        json!({"otherConfig": "key1: value1", "someConfig": "key2: value2"}),
        json!({})
    )]
    fn test_agent_configs_iter(
        #[case] config_map: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let opamp_config = testing_agent_config(config_map);

        let result: HashMap<&String, &String> = opamp_config.agent_configs_iter().collect();
        let expected: HashMap<String, String> = serde_json::from_value(expected).unwrap();

        assert_eq!(result.len(), expected.len());
        for (expected_key, expected_value) in &expected {
            assert_eq!(
                result.get(expected_key).map(|v| v.as_str()),
                Some(expected_value.as_str())
            );
        }
    }

    #[rstest]
    #[case::no_override(json!({"agentConfig": "key: value"}), None)]
    #[case::no_suffix(json!({"agentConfig": "key: value", "override.agentConfig": "key: value2"}), Some("key: value2"))]
    #[case::suffix(json!({"agentConfig": "key: value", "override.agentConfig-1": "key: value2"}), Some("key: value2"))]
    fn test_overrides_blob(#[case] config_map: serde_json::Value, #[case] expected: Option<&str>) {
        let opamp_config = testing_agent_config(config_map);
        let overrides = opamp_config.overrides().expect("no error expected");
        assert_eq!(overrides.blob().map(|v| v.as_str()), expected);
    }

    #[test]
    fn test_overrides_blob_multiple_errors() {
        let opamp_config = testing_agent_config(
            json!({"override.agentConfig": "key: value", "override.agentConfig-1": "key: value1"}),
        );
        let result = opamp_config.overrides();
        assert_matches!(result, Err(OpampRemoteConfigError::InvalidConfig(_, s)) => {
            assert!(s.contains("multiple configurations with 'override.agentConfig' prefix"));
        });
    }

    #[rstest]
    #[case::single_override(
        json!({"variable.agentConfig.key1": "value1"}),
        json!({"key1": "value1"})
    )]
    #[case::nested_override(
        json!({"variable.agentConfig.foo.bar": "value1"}),
        json!({"foo.bar": "value1"})
    )]
    #[case::multiple_overrides(
        json!({"variable.agentConfig.key1": "value1", "variable.agentConfig.key2": "value2"}),
        json!({"key1": "value1", "key2": "value2"})
    )]
    #[case::ignores_non_matching(
        json!({"agentConfig": "key: value", "override.agentConfig": "key: value2", "variable.agentConfig.key1": "value1"}),
        json!({"key1": "value1"})
    )]
    #[case::ignores_malformed_missing_separator(
        json!({"variable.agentConfig": "value1"}),
        json!({})
    )]
    #[case::ignores_map_entry_overrides(
        json!({"variable.agentConfig.key1": "value1", "variable.agentConfig.config_integrations:file1.yaml": "content: whatever-1"}),
        json!({"key1": "value1"})
    )]
    fn test_overrides_variables(
        #[case] config_map: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let opamp_config = testing_agent_config(config_map);

        let result: HashMap<&str, &str> = opamp_config
            .overrides()
            .expect("no error expected")
            .variables()
            .iter()
            .map(|variable_override| (variable_override.path(), variable_override.raw_value()))
            .collect();
        let expected: HashMap<String, String> = serde_json::from_value(expected).unwrap();

        assert_eq!(result.len(), expected.len());
        for (expected_key, expected_value) in &expected {
            assert_eq!(
                result.get(expected_key.as_str()).copied(),
                Some(expected_value.as_str())
            );
        }
    }

    #[rstest]
    #[case::map_entry_override(
        json!({"variable.agentConfig.config_integrations:file1.yaml": "content: whatever-1"}),
        json!({"config_integrations": {"file1.yaml": "content: whatever-1"}})
    )]
    #[case::map_entry_nested_path(
        json!({"variable.agentConfig.foo.bar:baz.yaml": "value1"}),
        json!({"foo.bar": {"baz.yaml": "value1"}})
    )]
    #[case::map_entry_only_first_colon_is_delimiter(
        json!({"variable.agentConfig.key1:file:name.yaml": "value1"}),
        json!({"key1": {"file:name.yaml": "value1"}})
    )]
    #[case::ignores_whole_variable_overrides(
        json!({"variable.agentConfig.key1": "value1", "variable.agentConfig.config_integrations:file1.yaml": "content: whatever-1"}),
        json!({"config_integrations": {"file1.yaml": "content: whatever-1"}})
    )]
    #[case::ignores_malformed_missing_separator(
        json!({"variable.agentConfig": "value1"}),
        json!({})
    )]
    fn test_overrides_map_entries(
        #[case] config_map: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let opamp_config = testing_agent_config(config_map);

        let result: HashMap<&str, HashMap<&str, &str>> = opamp_config
            .overrides()
            .expect("no error expected")
            .map_entries()
            .iter()
            .fold(HashMap::new(), |mut acc, map_entry_override| {
                acc.entry(map_entry_override.path())
                    .or_default()
                    .insert(map_entry_override.map_key(), map_entry_override.raw_value());
                acc
            });
        let expected: HashMap<String, HashMap<String, String>> =
            serde_json::from_value(expected).unwrap();

        assert_eq!(result.len(), expected.len());
        for (expected_path, expected_entries) in &expected {
            let actual_entries = result
                .get(expected_path.as_str())
                .expect("variable path not found");
            assert_eq!(actual_entries.len(), expected_entries.len());
            for (expected_key, expected_value) in expected_entries {
                assert_eq!(
                    actual_entries.get(expected_key.as_str()).copied(),
                    Some(expected_value.as_str())
                );
            }
        }
    }

    #[test]
    fn test_overrides_map_entry_empty_key_errors() {
        let opamp_config = testing_agent_config(json!({"variable.agentConfig.key1:": "value1"}));
        let result = opamp_config.overrides();
        assert_matches!(result, Err(OpampRemoteConfigError::InvalidConfig(_, s)) => {
            assert!(s.contains("empty map-entry key for override variable 'key1'"));
        });
    }
}
