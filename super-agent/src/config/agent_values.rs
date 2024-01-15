use serde::de::Error;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use thiserror::Error;

use super::agent_type::agent_types::{AgentVariables, FinalAgent};
use super::agent_type::error::AgentTypeError;
use super::agent_type::runtime_config_templates::TEMPLATE_KEY_SEPARATOR;
use super::agent_type::trivial_value::{FilePathWithContent, Number, TrivialValue};
use super::agent_type::variable_spec::kind::Kind;
use super::agent_type::variable_spec::kind_value::KindValue;
use super::agent_type::variable_spec::spec::Spec;

use crate::config::agent_type::variable_spec::spec::EndSpec;
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
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct AgentValues(serde_yaml::Value);

#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct NormalizedValues(HashMap<String, TrivialValue>);

#[derive(Error, Debug)]
pub enum AgentValuesError {
    #[error("invalid agent values format: `{0}`")]
    FormatError(#[from] serde_yaml::Error),
}

impl AgentValues {
    /// get_from_normalized recursively searches for a TrivialValue given a normalized prefix. A
    /// normalized prefix flattens a Map path in a single string in which each indirection is
    /// denoted with the TEMPLATE_KEY_SEPARATOR.
    /// If found, an owned value will be returned.
    // pub(crate) fn get_from_normalized(&self, normalized_prefix: &str) -> Option<TrivialValue> {
    //     get_from_normalized(&self.0, normalized_prefix)
    // }

    /// normalize_with_agent_type verifies that all required Agent variables are defined in the
    /// SubAgentConfig and transforms the types with check_type
    pub(crate) fn normalize_with_agent_type(
        mut self,
        agent_type: &FinalAgent,
    ) -> Result<NormalizedValues, AgentTypeError> {
        // let mut normalized_values = HashMap::default();
        // for (k, v) in agent_type.variables.0.iter() {
        //     let value = get_from_normalized(&self.0, k);

        //     // required value but not defined in SubAgentConfig
        //     if value.is_none() && v.kind.is_required() {
        //         return Err(AgentTypeError::MissingAgentKey(k.clone()));
        //     }

        //     // check type matches agent one and apply transformations
        //     if let Some(inner) = value {
        //         let _ = update_from_normalized(&mut self.0, k, inner.clone().check_type(v)?);
        //     }
        // }

        Ok(NormalizedValues::default())
    }
}

fn update_specs(
    values: serde_yaml::Value,
    agent_vars: &mut HashMap<String, Spec>,
) -> Result<(), AgentTypeError> {
    let values: HashMap<String, Value> = serde_yaml::from_value(values)?;
    for (ref k, v) in values.into_iter() {
        let mut spec = agent_vars
            .get_mut(k)
            .ok_or_else(|| AgentTypeError::MissingAgentKey(k.clone()))?;

        match spec {
            Spec::SpecEnd(EndSpec { kind, .. }) => kind.from_yaml_value(v)?,
            Spec::SpecMapping(m) => update_specs(v, m)?,
        }
    }
    Ok(())
}

// fn normalize_values(
//     values: serde_yaml::Value,
//     agent_vars: &HashMap<String, Spec>,
// ) -> Result<HashMap<String, TrivialValue>, AgentTypeError> {
//     let mut updated_spec = agent_vars.clone();
//     let mut normalized_values = HashMap::default();

//     // Attempt to get a HashMap<String, serde_yaml::Value> from the values and compare it with agent_vars
//     let values: HashMap<String, Value> = serde_yaml::from_value(values)?;
//     for (k, v) in values.into_iter() {
//         let mut spec = agent_vars
//             .get_mut(&k)
//             .ok_or(AgentTypeError::MissingAgentKey(k.clone()))?;

//         match spec {
//             Spec::SpecEnd(EndSpec { kind, .. }) => {
//                 kind.from_yaml_value(v)?
//             }
//             Spec::SpecMapping(m) =>
//         }

//     }

//     Ok(normalized_values)
// }

impl TryFrom<String> for AgentValues {
    type Error = AgentValuesError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(serde_yaml::from_str::<AgentValues>(value.as_str())?)
    }
}

/// get_from_normalized recursively searches for a TrivialValue given a normalized prefix.
// fn get_from_normalized(
//     inner: &Map<String, TrivialValue>,
//     normalized_prefix: &str,
// ) -> Option<TrivialValue> {
//     let prefix = normalized_prefix.split_once(TEMPLATE_KEY_SEPARATOR);
//     if let Some((key, suffix)) = prefix {
//         if let Some(TrivialValue::Map(inner_map)) = inner.get(key) {
//             return get_from_normalized(inner_map, suffix);
//         }
//     } else {
//         return inner.get(normalized_prefix).cloned();
//     }
//     None
// }

/// update_from_normalized updates a TrivialValue given a normalized prefix.
// fn update_from_normalized(
//     inner: &mut HashMap<String, TrivialValue>,
//     normalized_prefix: &str,
//     value: TrivialValue,
// ) -> Option<TrivialValue> {
//     let prefix = normalized_prefix.split_once(TEMPLATE_KEY_SEPARATOR);
//     if let Some((key, suffix)) = prefix {
//         if let Some(TrivialValue::Map(inner_map)) = inner.get_mut(key) {
//             return update_from_normalized(inner_map, suffix, value);
//         }
//     } else {
//         return inner.insert(normalized_prefix.to_string(), value);
//     }
//     None
// }

#[cfg(test)]
mod tests {

    use crate::config::agent_type::trivial_value::{FilePathWithContent, Number};

    use super::*;

    // impl AgentValues {
    //     pub fn new(values: HashMap<String, TrivialValue>) -> Self {
    //         Self(values)
    //     }
    // }

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
        let expected: Map<String, TrivialValue> = Map::from([
            (
                "description".to_string(),
                TrivialValue::Map(Map::from([
                    (
                        "name".to_string(),
                        TrivialValue::String("newrelic-infra".to_string()),
                    ),
                    (
                        "float_val".to_string(),
                        TrivialValue::Number(Number::Float(0.14)),
                    ),
                    ("logs".to_string(), TrivialValue::Number(Number::NegInt(-4))),
                ])),
            ),
            (
                "configuration".to_string(),
                TrivialValue::String(
                    r#"license: abc123
staging: true
extra_list:
  key: value
  key2: value2
"#
                    .to_string(),
                ),
            ),
            (
                "config".to_string(),
                TrivialValue::Map(Map::from([(
                    "envs".to_string(),
                    TrivialValue::Map(Map::from([
                        (
                            "name".to_string(),
                            TrivialValue::String("newrelic-infra".to_string()),
                        ),
                        (
                            "name2".to_string(),
                            TrivialValue::String("newrelic-infra2".to_string()),
                        ),
                    ])),
                )])),
            ),
            ("verbose".to_string(), TrivialValue::Bool(true)),
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
    fn test_update_specs() {
        let input_structure = serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let mut agent_type =
            serde_yaml::from_str::<FinalAgent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let expected = HashMap::from([
            (
                "deployment".to_string(),
                Spec::SpecMapping(HashMap::from([(
                    "on_host".to_string(),
                    Spec::SpecMapping(HashMap::from([
                        (
                            "path".to_string(),
                            Spec::SpecEnd(EndSpec {
                                description: "Path to the agent".to_string(),
                                kind: Kind::String(KindValue {
                                    required: true,
                                    default: None,
                                    final_value: None,
                                    file_path: None,
                                }),
                            }),
                        ),
                        (
                            "args".to_string(),
                            Spec::SpecEnd(EndSpec {
                                description: "Args passed to the agent".to_string(),
                                kind: Kind::String(KindValue {
                                    required: true,
                                    default: None,
                                    final_value: None,
                                    file_path: None,
                                }),
                            }),
                        ),
                    ])),
                )])),
            ),
            (
                "config".to_string(),
                Spec::SpecEnd(EndSpec {
                    description: "Path to the agent".to_string(),
                    kind: Kind::File(KindValue {
                        required: true,
                        default: None,
                        final_value: None,
                        file_path: Some("newrelic-infra.yml".into()),
                    }),
                }),
            ),
            (
                "integrations".to_string(),
                Spec::SpecEnd(EndSpec {
                    description: "Newrelic integrations configuration yamls".to_string(),
                    kind: Kind::MapStringFile(KindValue {
                        required: true,
                        default: None,
                        final_value: None,
                        file_path: Some("integrations.d".into()),
                    }),
                }),
            ),
        ]);

        update_specs(input_structure.0, &mut agent_type.variables.0).unwrap();

        assert_eq!(expected, agent_type.variables.0);
    }

    #[test]
    fn test_validate_with_agent_type() {
        let input_structure = serde_yaml::from_str::<AgentValues>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let agent_type = serde_yaml::from_str::<FinalAgent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let expected = Map::from([
            (
                "deployment".to_string(),
                TrivialValue::Map(Map::from([(
                    "on_host".to_string(),
                    TrivialValue::Map(Map::from([
                        (
                            "args".to_string(),
                            TrivialValue::String("--verbose true".to_string()),
                        ),
                        ("path".to_string(), TrivialValue::String("/etc".to_string())),
                    ])),
                )])),
            ),
            (
                "config".to_string(),
                TrivialValue::File(FilePathWithContent::new(
                    "newrelic-infra.yml".to_string(),
                    "test".to_string(),
                )),
            ),
            (
                "integrations".to_string(),
                TrivialValue::Map(Map::from([(
                    "kafka".to_string(),
                    TrivialValue::File(FilePathWithContent::new(
                        "integrations.d".to_string(),
                        "strategy: bootstrap\n".to_string(),
                    )),
                )])),
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
            "Type mismatch while parsing. Expected type String, got value Bool(true)"
        );
    }
}
