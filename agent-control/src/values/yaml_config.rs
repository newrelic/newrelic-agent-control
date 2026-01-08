use crate::agent_control::config::AgentControlDynamicConfig;
use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use thiserror::Error;

/// The YAMLConfig represent any YAML config that the AgentControl can read and store.
/// It enforces that the root of the tree is a hashmap and not an array or a single element.
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct YAMLConfig(HashMap<String, serde_yaml::Value>);

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
}

#[derive(Error, Debug)]
#[error("{0}")]
pub struct YAMLConfigError(pub String);

impl Templateable for YAMLConfig {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self(self.0.template_with(variables)?))
    }
}

impl Templateable for HashMap<String, serde_yaml::Value> {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|(key, v)| Ok((key, v.template_with(variables)?)))
            .collect()
    }
}

impl From<YAMLConfig> for HashMap<String, serde_yaml::Value> {
    fn from(values: YAMLConfig) -> Self {
        values.0
    }
}

impl TryFrom<&AgentControlDynamicConfig> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: &AgentControlDynamicConfig) -> Result<Self, Self::Error> {
        serde_yaml::from_value(
            serde_yaml::to_value(value)
                .map_err(|e| YAMLConfigError(format!("serializing dynamic config: {e}")))?,
        )
        .map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}

impl TryFrom<String> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        serde_yaml::from_str::<YAMLConfig>(value.as_str())
            .map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}
impl TryFrom<&str> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        serde_yaml::from_str::<YAMLConfig>(value)
            .map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}

impl TryFrom<YAMLConfig> for String {
    type Error = YAMLConfigError;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        //serde_yaml::to_string returns "{}\n" if value is empty
        if value.0.is_empty() {
            return Ok("".to_string());
        }
        serde_yaml::to_string(&value).map_err(|e| YAMLConfigError(format!("decoding config: {e}")))
    }
}

pub fn has_remote_management(capabilities: &Capabilities) -> bool {
    capabilities.has_capability(AgentCapabilities::AcceptsRemoteConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST,
        agent_type::{
            definition::AgentType,
            variable::{Variable, tree::Tree},
        },
    };
    use rstest::rstest;
    use serde_json::json;
    use serde_yaml::{Mapping, Value};

    impl YAMLConfig {
        pub(crate) fn new(values: HashMap<String, Value>) -> Self {
            Self(values)
        }

        pub(crate) fn get(&self, key: &str) -> Option<&Value> {
            self.0.get(key)
        }
    }

    const EXAMPLE_CONFIG: &str = r#"
description:
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
        let actual = serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG);

        assert!(actual.is_ok());
    }

    #[test]
    fn test_yaml_config() {
        let actual = serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG).unwrap();
        let expected = Value::Mapping(Mapping::from_iter([
            (
                Value::String("description".to_string()),
                Value::Mapping(Mapping::from_iter([
                    (
                        Value::String("name".to_string()),
                        Value::String("newrelic-infra".to_string()),
                    ),
                    (
                        Value::String("float_val".to_string()),
                        Value::Number(serde_yaml::Number::from(0.14_f64)),
                    ),
                    (
                        Value::String("logs".to_string()),
                        Value::Number(serde_yaml::Number::from(-4_i64)),
                    ),
                ])),
            ),
            (
                Value::String("configuration".to_string()),
                Value::String(
                    "license: abc123\nstaging: true\nextra_list:\n  key: value\n  key2: value2\n"
                        .to_string(),
                ),
            ),
            (
                Value::String("config".to_string()),
                Value::Mapping(Mapping::from_iter([(
                    Value::String("envs".to_string()),
                    Value::Mapping(Mapping::from_iter([
                        (
                            Value::String("name".to_string()),
                            Value::String("newrelic-infra".to_string()),
                        ),
                        (
                            Value::String("name2".to_string()),
                            Value::String("newrelic-infra2".to_string()),
                        ),
                    ])),
                )])),
            ),
            (Value::String("verbose".to_string()), Value::Bool(true)),
        ]));

        assert_eq!(actual.0, serde_yaml::from_value(expected).unwrap());
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
variables:
  common:
    whatever:
      test:
        path:
          description: "Path to the agent"
          type: string
          required: true
        args:
          description: "Args passed to the agent"
          type: string
          required: true
deployment:
  linux: {}
  windows: {}
"#;

    #[test]
    fn test_update_specs() {
        let input_structure = serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let agent_type =
            AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE, &AGENT_CONTROL_MODE_ON_HOST);

        let expected = HashMap::from([(
            "whatever".to_string(),
            Tree::Mapping(HashMap::from([(
                "test".to_string(),
                Tree::Mapping(HashMap::from([
                    (
                        "path".to_string(),
                        Tree::End(Variable::new_string(
                            "Path to the agent".to_string(),
                            true,
                            None,
                            Some("/etc".to_string()),
                        )),
                    ),
                    (
                        "args".to_string(),
                        Tree::End(Variable::new_string(
                            "Args passed to the agent".to_string(),
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
            serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE_WRONG_TYPE).unwrap();
        let agent_type =
            AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE, &AGENT_CONTROL_MODE_ON_HOST);

        let result = agent_type.variables.fill_with_values(input_structure);

        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "error while parsing: invalid type: boolean `true`, expected a string"
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
}
