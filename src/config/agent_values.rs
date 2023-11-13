use serde::Deserialize;
use std::collections::HashMap as Map;

use super::agent_type::agent_types::FinalAgent;
use super::agent_type::error::AgentTypeError;
use super::agent_type::runtime_config_templates::TEMPLATE_KEY_SEPARATOR;
use super::agent_type::trivial_value::TrivialValue;

/// User-provided config.
///
/// User-provided configuration (normally via a YAML file) that must follow the tree-like structure of [`Agent`]'s [`variables`] and will be used to populate the [`Agent`]'s [ `runtime_config`] field to totally define a deployable Sub Agent.
///
/// The below example in YAML format:
///
/// ```yaml
/// system:
///  logging:
///    level: debug
///
///
/// custom_envs:
///   file: /tmp/aux.txt
/// ```
///
/// Coupled with a specification of an agent type like this one:
///
/// ```yaml
/// name: nrdot
/// namespace: newrelic
/// version: 0.1.0
///
/// variables:
///  system:
///   logging:
///     level:
///      description: "Logging level"
///      type: string
///      required: true
///  custom_envs:
///     description: "Logging level"
///     type: map[string]string
///     required: true
///
/// deployment:
///   on_host:
///     executables:
///       - path: "/etc/otelcol"
///         args: "--log-level debug"
///         env: "{custom_envs}"
///     # the health of nrdot is determined by whether the agent process
///     # is up and alive
///     health:
///       strategy: process
/// ```
///
/// Will produce the following end result:
///
/// ```yaml
/// name: nrdot
/// namespace: newrelic
/// version: 0.1.0
///
/// variables:
///   system:
///     logging:
///       level:
///         description: "Logging level"
///         type: string
///         required: true
///         default:
///         final_value: debug
///
/// deployment:
///   on_host:
///     executables:
///       - path: "/etc/otelcol"
///         args: "--log-level debug"
///     # the health of nrdot is determined by whether the agent process
///     # is up and alive
///     health:
///       strategy: process
/// ```
///
/// Please see the tests in the sources for more examples.
///
/// [agent_type]: crate::config::agent_type
#[derive(Debug, PartialEq, Deserialize, Clone, Default)]
pub struct AgentValues(Map<String, AgentValuesInner>);

#[derive(Debug, PartialEq, Deserialize, Clone)]
#[serde(untagged)]
enum AgentValuesInner {
    End(TrivialValue),
    Nesting(AgentValues),
}

impl AgentValues {
    /// get_from_normalized recursively searches for a TrivialValue given a normalized prefix.  A
    /// normalized prefix flattens a Map path in a single string in which each indirection is
    /// denoted with the TEMPLATE_KEY_SEPARATOR.
    /// If found, an owned value will be returned.
    pub(crate) fn get_from_normalized(&self, normalized_prefix: &str) -> Option<TrivialValue> {
        // FIXME should we return a Result to account for unsanitized inputs? For example "some.key." (trailing dot)
        let split = normalized_prefix.split_once(TEMPLATE_KEY_SEPARATOR);
        let Some((prefix, suffix)) = split else {
            // if there is no TEMPLATE_KEY_SEPARATOR, we are at the end of the recursive search,
            // we can return a value only if we are at an End of the AgentValues tree.
            return match self.0.get(normalized_prefix) {
                Some(AgentValuesInner::End(v)) => Some(v.clone()),
                _ => None,
            };
        };

        match self.0.get(prefix) {
            None => None,
            Some(AgentValuesInner::End(v)) => Some(v.clone()),
            Some(AgentValuesInner::Nesting(m)) => m.get_from_normalized(suffix),
        }
    }

    /// normalize_with_agent_type verifies that all required Agent variables are defined in the
    /// SubAgentConfig and transforms the types with check_type
    pub(crate) fn normalize_with_agent_type(
        mut self,
        agent_type: &FinalAgent,
    ) -> Result<Self, AgentTypeError> {
        for (k, v) in agent_type.variables.iter() {
            let value = self.get_from_normalized(k);

            // required value but not defined in SubAgentConfig
            if value.is_none() && v.kind.required() {
                return Err(AgentTypeError::MissingAgentKey(k.clone()));
            }

            // check type matches agent one and apply transformations
            if let Some(inner) = value {
                self.update_from_normalized(k, inner.clone().check_type(v)?);
            }
        }

        Ok(self)
    }

    /// update_from_normalized updates a TrivialValue given a normalized prefix.
    fn update_from_normalized(&mut self, normalized_prefix: &str, value: TrivialValue) {
        let split = normalized_prefix.split_once(TEMPLATE_KEY_SEPARATOR);
        let Some((prefix, suffix)) = split else {
            // if there is no TEMPLATE_KEY_SEPARATOR, we are at the end of the recursive search,
            // we can update a value only if we are at an End of the AgentValues tree.
            self.0
                .insert(normalized_prefix.to_owned(), AgentValuesInner::End(value));
            return;
        };

        match self.0.get_mut(prefix) {
            None => (),
            Some(AgentValuesInner::End(v)) => *v = value,
            Some(AgentValuesInner::Nesting(m)) => m.update_from_normalized(suffix, value),
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::config::agent_type::trivial_value::{FilePathWithContent, Number};

    use super::*;

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
        let actual = serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG);

        assert!(actual.is_ok());
    }

    #[test]
    fn test_agent_values() {
        let actual = serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG).unwrap();
        let expected: Map<String, AgentValuesInner> = Map::from([
            (
                "description".to_owned(),
                AgentValuesInner::Nesting(AgentValues(Map::from([
                    (
                        "name".to_owned(),
                        AgentValuesInner::End(TrivialValue::String("newrelic-infra".to_owned())),
                    ),
                    (
                        "float_val".to_owned(),
                        AgentValuesInner::End(TrivialValue::Number(Number::Float(0.14))),
                    ),
                    (
                        "logs".to_owned(),
                        AgentValuesInner::End(TrivialValue::Number(Number::NegInt(-4))),
                    ),
                ]))),
            ),
            (
                "configuration".to_owned(),
                AgentValuesInner::End(TrivialValue::String(
                    r#"license: abc123
staging: true
extra_list:
  key: value
  key2: value2
"#
                    .to_owned(),
                )),
            ),
            (
                "config".to_owned(),
                AgentValuesInner::Nesting(AgentValues(Map::from([(
                    "envs".to_owned(),
                    AgentValuesInner::End(TrivialValue::MapStringString(Map::from([
                        ("name".to_string(), "newrelic-infra".to_string()),
                        ("name2".to_string(), "newrelic-infra2".to_string()),
                    ]))),
                )]))),
            ),
            (
                "verbose".to_string(),
                AgentValuesInner::End(TrivialValue::Bool(true)),
            ),
        ]);

        assert_eq!(actual.0, expected);
    }

    const EXAMPLE_CONFIG_REPLACE: &str = r#"
deployment:
  on_host:
    path: "/etc"
    args: --verbose true
config: test
integrations:
  kafka: |
    strategy: bootstrap
"#;
    const EXAMPLE_AGENT_YAML_REPLACE: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
  config:
    description: "Path to the agent"
    type: file
    required: true
    file_path: "newrelic-infra.yml"
  deployment:
    on_host:
      path:
        description: "Path to the agent"
        type: string
        required: true
      args:
        description: "Args passed to the agent"
        type: string
        required: true
  integrations:
    description: "Newrelic integrations configuration yamls"
    type: map[string]file
    required: true
    file_path: integrations.d
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.path}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;

    #[test]
    fn test_validate_with_agent_type() {
        let input_structure = serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let agent_type = serde_yaml::from_str::<FinalAgent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let expected = Map::from([
            (
                "deployment".to_string(),
                AgentValuesInner::Nesting(AgentValues(Map::from([(
                    "on_host".to_string(),
                    AgentValuesInner::Nesting(AgentValues(Map::from([
                        (
                            "args".to_string(),
                            AgentValuesInner::End(TrivialValue::String(
                                "--verbose true".to_string(),
                            )),
                        ),
                        (
                            "path".to_string(),
                            AgentValuesInner::End(TrivialValue::String("/etc".to_string())),
                        ),
                    ]))),
                )]))),
            ),
            (
                "config".to_string(),
                AgentValuesInner::End(TrivialValue::File(FilePathWithContent::new(
                    "newrelic-infra.yml".to_string(),
                    "test".to_string(),
                ))),
            ),
            (
                "integrations".to_string(),
                AgentValuesInner::Nesting(AgentValues(Map::from([(
                    "kafka".to_string(),
                    AgentValuesInner::End(TrivialValue::File(FilePathWithContent::new(
                        "integrations.d".to_string(),
                        "strategy: bootstrap\n".to_string(),
                    ))),
                )]))),
            ),
        ]);
        let actual = input_structure
            .normalize_with_agent_type(&agent_type)
            .unwrap();

        assert_eq!(expected, actual.0);
    }

    const EXAMPLE_CONFIG_REPLACE_NOPATH: &str = r#"
    deployment:
      on_host:
        args: --verbose true
    integrations: {}
    config: test
    "#;

    #[test]
    fn test_validate_with_agent_type_missing_required() {
        let input_structure =
            serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG_REPLACE_NOPATH).unwrap();
        let agent_type = serde_yaml::from_str::<FinalAgent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let actual = input_structure.normalize_with_agent_type(&agent_type);

        assert!(actual.is_err());
        assert_eq!(
            format!("{}", actual.unwrap_err()),
            "Missing required key in config: `deployment.on_host.path`"
        );
    }

    const EXAMPLE_CONFIG_REPLACE_WRONG_TYPE: &str = r#"
    config: test
    deployment:
      on_host:
        path: true
        args: --verbose true
    integrations: {}
    "#;

    #[test]
    fn test_validate_with_agent_type_wrong_value_type() {
        let input_structure =
            serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG_REPLACE_WRONG_TYPE).unwrap();
        let agent_type = serde_yaml::from_str::<FinalAgent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let actual = input_structure.normalize_with_agent_type(&agent_type);

        assert!(actual.is_err());
        assert_eq!(
            format!("{}", actual.unwrap_err()),
            "Type mismatch while parsing. Expected type string, got value Bool(true)"
        );
    }
}
