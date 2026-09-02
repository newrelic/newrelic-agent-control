//! The [`YAMLConfig`] type wrapping a YAML mapping that Agent Control can read and store.

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use crate::{
    agent_control::config::AgentControlDynamicConfig, agent_type::templates::TEMPLATE_KEY_SEPARATOR,
};
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

/// The YAMLConfig represent any YAML config that the AgentControl can read and store.
/// It enforces that the root of the tree is a hashmap and not an array or a single element.
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct YAMLConfig(HashMap<String, Value>);

impl YAMLConfig {
    /// Returns true if the YAMLConfig is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Removes a key from the YAMLConfig returning it if it exists.
    pub fn remove_key(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Tries to append one YAMLConfig into another, returning an error if there are any duplicate keys.
    ///
    /// # Errors
    /// Returns an error if there are any duplicate keys between the two YAMLConfig instances.
    pub fn try_append(a: Self, b: Self) -> Result<Self, YAMLConfigError> {
        let mut result = a;
        for (key, value) in b.0 {
            if result.0.contains_key(&key) {
                return Err(YAMLConfigError(format!(
                    "cannot append duplicated key: {}",
                    key
                )));
            }
            result.0.insert(key, value);
        }
        Ok(result)
    }

    /// Merges the provided [YAMLConfig] values, `b` values take precede.
    ///
    /// # Example
    /// ```
    /// # use newrelic_agent_control::values::yaml_config::YAMLConfig;
    /// # use serde_json::json;
    /// let a: YAMLConfig = serde_json::from_value(json!({"key1": "value1", "key2": {"x": "y"}})).unwrap();
    /// let b: YAMLConfig = serde_json::from_value(json!({"key2": "value2", "key3": "value3"})).unwrap();
    /// let merged = YAMLConfig::merge_override(a, b);
    /// assert_eq!(merged, serde_json::from_value(json!({"key1": "value1", "key2": "value2", "key3": "value3"})).unwrap());
    /// ```
    pub fn merge_override(a: Self, b: Self) -> Self {
        b.0.into_iter().fold(a, |mut result, (k, v)| {
            result.0.insert(k, v);
            result
        })
    }

    /// Sets `value` at the given dot-separated `variable_path`, creating any missing intermediate
    /// mappings and overwriting the value at the final segment.
    ///
    /// # Example
    /// ```
    /// # use newrelic_agent_control::values::yaml_config::YAMLConfig;
    /// # use serde_json::json;
    /// let mut config: YAMLConfig = serde_json::from_value(json!({"foo": {"bar": "value1"}})).unwrap();
    /// config.override_variable_value("foo.bar", json!("overridden")).unwrap();
    /// config.override_variable_value("foo.baz", json!("new")).unwrap();
    /// assert_eq!(config, serde_json::from_value(json!({"foo": {"bar": "overridden", "baz": "new"}})).unwrap());
    /// ```
    /// # Errors
    /// Returns an error if `variable_path` is empty, or if an intermediate segment already holds a
    /// non-mapping value.
    ///
    /// ```
    /// # use newrelic_agent_control::values::yaml_config::YAMLConfig;
    /// # use serde_json::json;
    /// let mut config: YAMLConfig = serde_json::from_value(json!({"foo": {"bar": "value1"}})).unwrap();
    /// let err = config.override_variable_value("foo.bar.baz", json!("new")).unwrap_err();
    /// assert_eq!(
    ///     err.to_string(),
    ///     "cannot override nested variable path 'foo.bar.baz': segment 'foo.bar' is not a mapping"
    /// );
    /// ```
    pub fn override_variable_value(
        &mut self,
        variable_path: &str,
        value: Value,
    ) -> Result<(), YAMLConfigError> {
        let Some((parent_path, last_segment)) = variable_path.rsplit_once(TEMPLATE_KEY_SEPARATOR)
        else {
            if variable_path.is_empty() {
                return Err(YAMLConfigError(
                    "cannot override an empty variable path".to_string(),
                ));
            }
            // Overrides a single variable case.
            self.0.insert(variable_path.to_string(), value);
            return Ok(());
        };

        let Value::Object(map) = self.get_or_insert_mut(parent_path).map_err(|err| {
            YAMLConfigError(format!(
                "cannot override nested variable path '{variable_path}': {err}"
            ))
        })?
        else {
            return Err(YAMLConfigError(format!(
                "cannot override nested variable path '{variable_path}': segment '{parent_path}' is not a mapping"
            )));
        };
        map.insert(last_segment.to_string(), value);

        Ok(())
    }

    /// Sets the value `value` inside the specific mapping (`map_key`) living at the given dot-separated
    /// `variable_path`, creating any missing intermediate mappings and preserving any other entries
    /// already present there.
    ///
    /// # Errors
    /// Returns an error if `variable_path` or `map_key` is empty, if an intermediate segment of
    /// `variable_path` already holds a non-mapping value, or if `variable_path` itself already
    /// holds a non-mapping value.
    pub fn override_variable_map_entry(
        &mut self,
        variable_path: &str,
        map_key: &str,
        value: Value,
    ) -> Result<(), YAMLConfigError> {
        if map_key.is_empty() {
            return Err(YAMLConfigError(
                "cannot override a map entry with an empty key".to_string(),
            ));
        }

        let Value::Object(map) = self.get_or_insert_mut(variable_path).map_err(|err| {
            YAMLConfigError(format!(
                "cannot set map entry '{map_key}' at variable path '{variable_path}': {err}"
            ))
        })?
        else {
            return Err(YAMLConfigError(format!(
                "cannot set map entry '{map_key}' at variable path '{variable_path}': value is not a mapping"
            )));
        };
        map.insert(map_key.to_string(), value);

        Ok(())
    }

    /// Walks the given dot-separated `variable_path`, creating any missing intermediate mappings,
    /// and returns a mutable reference to the [Value] living at that path.
    ///
    /// # Errors
    /// Returns an error if `variable_path` is empty, or if a segment along the way already holds
    /// a non-mapping value.
    fn get_or_insert_mut(&mut self, variable_path: &str) -> Result<&mut Value, YAMLConfigError> {
        let mut segments = variable_path.split(TEMPLATE_KEY_SEPARATOR);
        let first = segments
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| YAMLConfigError("cannot override an empty variable path".to_string()))?;

        let mut current = self
            .0
            .entry(first.to_string())
            .or_insert_with(|| Value::Object(Default::default()));
        let mut visited_path = first.to_string();

        for segment in segments {
            let Value::Object(map) = current else {
                return Err(YAMLConfigError(format!(
                    "cannot override nested variable path '{variable_path}': segment '{visited_path}' is not a mapping"
                )));
            };
            current = map
                .entry(segment.to_string())
                .or_insert_with(|| Value::Object(Default::default()));
            visited_path = format!("{visited_path}.{segment}");
        }

        Ok(current)
    }
}

/// Error produced while building, merging, or (de)serializing a [`YAMLConfig`].
#[derive(Error, Debug)]
#[error("{0}")]
pub struct YAMLConfigError(
    /// Human-readable description of the error.
    pub String,
);

impl Templateable for YAMLConfig {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self(self.0.template_with(variables)?))
    }
}

impl Templateable for HashMap<String, serde_json::Value> {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|(key, v)| Ok((key, v.template_with(variables)?)))
            .collect()
    }
}

impl From<YAMLConfig> for HashMap<String, serde_json::Value> {
    fn from(values: YAMLConfig) -> Self {
        values.0
    }
}

impl TryFrom<&AgentControlDynamicConfig> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: &AgentControlDynamicConfig) -> Result<Self, Self::Error> {
        serde_json::from_value(
            serde_json::to_value(value)
                .map_err(|e| YAMLConfigError(format!("serializing dynamic config: {e}")))?,
        )
        .map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}

fn parse_yaml_config(value: &str) -> Result<YAMLConfig, YAMLConfigError> {
    serde_saphyr::from_str(value).map_err(|e| {
        YAMLConfigError(format!(
            "decoding config: {}",
            e.render_with_formatter(&serde_saphyr::UserMessageFormatter)
        ))
    })
}

impl TryFrom<String> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_yaml_config(value.as_str())
    }
}
impl TryFrom<&str> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        parse_yaml_config(value)
    }
}

impl TryFrom<YAMLConfig> for String {
    type Error = YAMLConfigError;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        //serde_saphyr::to_string returns "{}\n" if value is empty
        if value.0.is_empty() {
            return Ok("".to_string());
        }
        serde_saphyr::to_string(&value)
            .map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}

/// Returns true if the given OpAMP capabilities accept remote configuration.
pub fn has_remote_management(capabilities: &Capabilities) -> bool {
    capabilities.has_capability(AgentCapabilities::AcceptsRemoteConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::{
        definition::AgentType,
        variable::{Variable, tree::Tree},
    };
    use rstest::rstest;
    use serde_json::json;
    use serde_json::{Map, Value};

    impl YAMLConfig {
        pub(crate) fn new(values: HashMap<String, Value>) -> Self {
            Self(values)
        }

        pub(crate) fn get(&self, key: &str) -> Option<&Value> {
            self.0.get(key)
        }
    }

    const EXAMPLE_CONFIG: &str = r#"
metadata:
  name: newrelic-infra
  float_val: 0.14
  logs: -4
configuration: |
  license: abc123
  staging: true
  extra_list:
    key: value
    key2: value2
config:
  envs:
    name: newrelic-infra
    name2: newrelic-infra2
verbose: true
"#;

    #[test]
    fn example_config() {
        let actual = serde_saphyr::from_str::<YAMLConfig>(EXAMPLE_CONFIG);

        assert!(actual.is_ok());
    }

    #[test]
    fn test_yaml_config() {
        let actual = serde_saphyr::from_str::<YAMLConfig>(EXAMPLE_CONFIG).unwrap();
        let expected = Value::Object(Map::from_iter([
            (
                "metadata".to_string(),
                Value::Object(Map::from_iter([
                    (
                        "name".to_string(),
                        Value::String("newrelic-infra".to_string()),
                    ),
                    (
                        "float_val".to_string(),
                        Value::Number(serde_json::Number::from_f64(0.14).unwrap()),
                    ),
                    (
                        "logs".to_string(),
                        Value::Number(serde_json::Number::from(-4_i64)),
                    ),
                ])),
            ),
            (
                "configuration".to_string(),
                Value::String(
                    "license: abc123\nstaging: true\nextra_list:\n  key: value\n  key2: value2\n"
                        .to_string(),
                ),
            ),
            (
                "config".to_string(),
                Value::Object(Map::from_iter([(
                    "envs".to_string(),
                    Value::Object(Map::from_iter([
                        (
                            "name".to_string(),
                            Value::String("newrelic-infra".to_string()),
                        ),
                        (
                            "name2".to_string(),
                            Value::String("newrelic-infra2".to_string()),
                        ),
                    ])),
                )])),
            ),
            ("verbose".to_string(), Value::Bool(true)),
        ]));

        assert_eq!(actual.0, serde_json::from_value(expected).unwrap());
    }

    const EXAMPLE_CONFIG_REPLACE: &str = r#"
whatever:
  test:
    path: "/etc"
    args: --verbose true
config: |
  test
integrations:
  kafka: |
    strategy: bootstrap
"#;
    const EXAMPLE_AGENT_YAML_REPLACE: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
platform: host
operating_system: linux
variables:
  whatever:
    test:
      path:
        type: string
        required: true
      args:
        type: string
        required: true
deployment: {}
"#;

    #[test]
    fn test_update_specs() {
        let input_structure = serde_saphyr::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let agent_type = AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE);

        let expected = HashMap::from([(
            "whatever".to_string(),
            Tree::Mapping(HashMap::from([(
                "test".to_string(),
                Tree::Mapping(HashMap::from([
                    (
                        "path".to_string(),
                        Tree::End(Variable::new_string(true, None, Some("/etc".to_string()))),
                    ),
                    (
                        "args".to_string(),
                        Tree::End(Variable::new_string(
                            true,
                            None,
                            Some("--verbose true".to_string()),
                        )),
                    ),
                ])),
            )])),
        )]);

        let filled_variables = agent_type
            .variables
            .fill_with_values(input_structure)
            .unwrap();

        assert_eq!(expected, filled_variables.0);
    }

    const EXAMPLE_CONFIG_REPLACE_WRONG_TYPE: &str = r#"
    config: |
      test
    whatever:
      test:
        path: true
        args: --verbose true
    integrations: {}
    "#;

    #[test]
    fn test_validate_with_agent_type_wrong_value_type() {
        let input_structure =
            serde_saphyr::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE_WRONG_TYPE).unwrap();
        let agent_type = AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE);

        let result = agent_type.variables.fill_with_values(input_structure);

        assert!(result.is_err());
        assert!(
            format!("{}", result.unwrap_err())
                .contains("invalid type: boolean `true`, expected a string")
        );
    }

    #[rstest]
    #[case::single_key_each(
        json!({"key1": "value1"}),
        json!({"key2": "value2"}),
        json!({"key1": "value1", "key2": "value2"})
    )]
    #[case::multiple_keys_no_overlap(
        json!({"key1": "value1", "key2": "value2"}),
        json!({"key3": "value3", "key4": "value4"}),
        json!({"key1": "value1", "key2": "value2", "key3": "value3", "key4": "value4"})
    )]
    #[case::empty(json!({}), json!({}), json!({}))]
    #[case::empty_first(json!({}), json!({"key1": "value1"}), json!({"key1": "value1"}))]
    #[case::empty_second(json!({"key1": "value1"}), json!({}), json!({"key1": "value1"}))]
    fn test_try_append_success(
        #[case] a: serde_json::Value,
        #[case] b: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let config_a = serde_json::from_value::<YAMLConfig>(a).unwrap();
        let config_b = serde_json::from_value::<YAMLConfig>(b).unwrap();
        let expected_config = serde_json::from_value::<YAMLConfig>(expected).unwrap();

        let result = YAMLConfig::try_append(config_a, config_b);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_config);
    }

    #[rstest]
    #[case::duplicate_key(json!({"key1": "value1"}), json!({"key1": "value2"}))]
    #[case::multiple_keys_with_duplicate(
        json!({"key1": "value1", "key2": "value2"}),
        json!({"key2": "value3", "key3": "value4"})
    )]
    fn test_try_append_duplicate_key_error(
        #[case] a: serde_json::Value,
        #[case] b: serde_json::Value,
    ) {
        let config_a = serde_json::from_value::<YAMLConfig>(a).unwrap();
        let config_b = serde_json::from_value::<YAMLConfig>(b).unwrap();

        let result = YAMLConfig::try_append(config_a, config_b);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .0
                .contains("cannot append duplicated key")
        );
    }

    #[rstest]
    #[case::single_key_each(
        json!({"key1": "value1"}),
        json!({"key2": "value2"}),
        json!({"key1": "value1", "key2": "value2"})
    )]
    #[case::multiple_keys_no_overlap(
        json!({"key1": "value1", "key2": "value2"}),
        json!({"key3": "value3", "key4": "value4"}),
        json!({"key1": "value1", "key2": "value2", "key3": "value3", "key4": "value4"})
    )]
    #[case::overlapping_keys_b_takes_precedence(
        json!({"key1": "value1", "key2": "value2"}),
        json!({"key2": "value3", "key3": "value4"}),
        json!({"key1": "value1", "key2": "value3", "key3": "value4"})
    )]
    #[case::all_overlapping_keys(
        json!({"key1": "value1", "key2": "value2"}),
        json!({"key1": "new1", "key2": "new2"}),
        json!({"key1": "new1", "key2": "new2"})
    )]
    #[case::empty(json!({}), json!({}), json!({}))]
    #[case::empty_first(json!({}), json!({"key1": "value1"}), json!({"key1": "value1"}))]
    #[case::empty_second(json!({"key1": "value1"}), json!({}), json!({"key1": "value1"}))]
    #[case::nested_objects_override(
        json!({"key1": "value1", "key2": {"x": "y"}}),
        json!({"key2": "value2", "key3": "value3"}),
        json!({"key1": "value1", "key2": "value2", "key3": "value3"})
    )]
    fn test_merge_override(
        #[case] a: serde_json::Value,
        #[case] b: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let config_a = serde_json::from_value::<YAMLConfig>(a).unwrap();
        let config_b = serde_json::from_value::<YAMLConfig>(b).unwrap();
        let expected_config = serde_json::from_value::<YAMLConfig>(expected).unwrap();

        let result = YAMLConfig::merge_override(config_a, config_b);
        assert_eq!(result, expected_config);
    }

    #[rstest]
    #[case::top_level_new_key(
        json!({"key1": "value1"}),
        "key2",
        json!("value2"),
        json!({"key1": "value1", "key2": "value2"})
    )]
    #[case::top_level_overwrite(
        json!({"key1": "value1"}),
        "key1",
        json!("overridden"),
        json!({"key1": "overridden"})
    )]
    #[case::nested_overwrite(
        json!({"foo": {"bar": "value1"}}),
        "foo.bar",
        json!("overridden"),
        json!({"foo": {"bar": "overridden"}})
    )]
    #[case::nested_new_key_sibling_untouched(
        json!({"foo": {"bar": "value1"}}),
        "foo.baz",
        json!("new"),
        json!({"foo": {"bar": "value1", "baz": "new"}})
    )]
    #[case::auto_creates_missing_intermediates(
        json!({}),
        "foo.bar.baz",
        json!("value"),
        json!({"foo": {"bar": {"baz": "value"}}})
    )]
    #[case::non_string_value(
        json!({}),
        "foo",
        json!({"nested": [1, 2]}),
        json!({"foo": {"nested": [1, 2]}})
    )]
    fn test_set_override_path_success(
        #[case] initial: serde_json::Value,
        #[case] path: &str,
        #[case] value: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let mut config = serde_json::from_value::<YAMLConfig>(initial).unwrap();
        let expected_config = serde_json::from_value::<YAMLConfig>(expected).unwrap();

        config.override_variable_value(path, value).unwrap();

        assert_eq!(config, expected_config);
    }

    #[rstest]
    #[case::empty_path(json!({"key1": "value1"}), "", json!("value"))]
    #[case::type_conflict_top_level(json!({"key1": "value1"}), "key1.nested", json!("value"))]
    #[case::type_conflict_deeper(json!({"foo": {"bar": "value1"}}), "foo.bar.baz", json!("value"))]
    fn test_set_override_path_error(
        #[case] initial: serde_json::Value,
        #[case] path: &str,
        #[case] value: serde_json::Value,
    ) {
        let mut config = serde_json::from_value::<YAMLConfig>(initial).unwrap();

        let result = config.override_variable_value(path, value);

        assert!(result.is_err());
    }

    #[rstest]
    #[case::creates_new_map(
        json!({}),
        "config_integrations",
        "file1.yaml",
        json!("content"),
        json!({"config_integrations": {"file1.yaml": "content"}})
    )]
    #[case::merges_preserving_siblings(
        json!({"config_integrations": {"file1.yaml": "old", "file3.yaml": "keep"}}),
        "config_integrations",
        "file1.yaml",
        json!("new"),
        json!({"config_integrations": {"file1.yaml": "new", "file3.yaml": "keep"}})
    )]
    #[case::nested_variable_path(
        json!({}),
        "foo.bar",
        "file1.yaml",
        json!("content"),
        json!({"foo": {"bar": {"file1.yaml": "content"}}})
    )]
    #[case::map_key_with_dot_stays_a_single_key(
        json!({}),
        "config_integrations",
        "file1.yaml",
        json!({"nested": "value"}),
        json!({"config_integrations": {"file1.yaml": {"nested": "value"}}})
    )]
    fn test_override_variable_map_entry_success(
        #[case] initial: serde_json::Value,
        #[case] variable_path: &str,
        #[case] map_key: &str,
        #[case] value: serde_json::Value,
        #[case] expected: serde_json::Value,
    ) {
        let mut config = serde_json::from_value::<YAMLConfig>(initial).unwrap();
        let expected_config = serde_json::from_value::<YAMLConfig>(expected).unwrap();

        config
            .override_variable_map_entry(variable_path, map_key, value)
            .unwrap();

        assert_eq!(config, expected_config);
    }

    #[rstest]
    #[case::empty_variable_path(json!({}), "", "file1.yaml", json!("value"))]
    #[case::empty_map_key(json!({}), "config_integrations", "", json!("value"))]
    #[case::intermediate_segment_not_a_mapping(
        json!({"foo": "scalar"}),
        "foo.bar",
        "file1.yaml",
        json!("value")
    )]
    #[case::final_value_not_a_mapping(
        json!({"config_integrations": "not-a-map"}),
        "config_integrations",
        "file1.yaml",
        json!("value")
    )]
    fn test_override_variable_map_entry_error(
        #[case] initial: serde_json::Value,
        #[case] variable_path: &str,
        #[case] map_key: &str,
        #[case] value: serde_json::Value,
    ) {
        let mut config = serde_json::from_value::<YAMLConfig>(initial).unwrap();

        let result = config.override_variable_map_entry(variable_path, map_key, value);

        assert!(result.is_err());
    }
}
