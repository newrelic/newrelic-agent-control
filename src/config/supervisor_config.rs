use serde::Deserialize;
use std::collections::HashMap as Map;

use crate::config::agent_type::SpecType;

use super::agent_type::AgentType;

type SupervisorConfig = Map<String, SupervisorConfigInner>;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum SupervisorConfigInner {
    NestedConfig(Map<String, SupervisorConfigInner>),
    EndValue(TrivialValue),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum TrivialValue {
    String(String),
    Bool(bool),
    Number(N),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum N {
    PosInt(u64),
    /// Always less than zero.
    NegInt(i64),
    /// May be infinite or NaN.
    Float(f64),
}

type NormalizedSupervisorConfig = Map<String, TrivialValue>;

fn normalize_supervisor_config(config: SupervisorConfig) -> NormalizedSupervisorConfig {
    let mut result = Map::new();
    config
        .into_iter()
        .for_each(|(k, v)| result.extend(inner_normalize(k, v)));
    result
}

fn inner_normalize(key: String, config: SupervisorConfigInner) -> NormalizedSupervisorConfig {
    let mut result = Map::new();
    match config {
        SupervisorConfigInner::NestedConfig(c) => c
            .into_iter()
            .for_each(|(k, v)| result.extend(inner_normalize(key.clone() + "." + &k, v))),
        SupervisorConfigInner::EndValue(v) => _ = result.insert(key, v),
    }
    result
}

fn validate_with_agent_type(
    config: NormalizedSupervisorConfig,
    agent_type: &AgentType,
) -> Result<NormalizedSupervisorConfig, String> {
    // What do we need to do?
    // Check that all the keys in the agent_type are present in the config
    // Also, check that all the values of the config are of the type declared in the config's NormalizedSpec
    let mut result: NormalizedSupervisorConfig = Map::new();
    let mut tmp_config = config.clone();

    for (k, v) in agent_type.spec.iter() {
        if !tmp_config.contains_key(k) && v.required {
            return Err(format!("Missing required key in config: {}", k));
        }

        if !tmp_config.contains_key(k) {
            // We have validated earlier that for a not `required` value, a default will be provided
            // so we can just unwrap.
            result.insert(k.clone(), v.default.clone().unwrap());
            continue;
        }

        // Check if the types match
        match tmp_config.get(k) {
            Some(s @ TrivialValue::String(_)) if v.type_ == SpecType::String => {
                _ = result.insert(k.clone(), s.clone())
            }
            Some(b @ TrivialValue::Bool(_)) if v.type_ == SpecType::Bool => {
                _ = result.insert(k.clone(), b.clone())
            }
            Some(n @ TrivialValue::Number(_)) if v.type_ == SpecType::Number => {
                _ = result.insert(k.clone(), n.clone())
            }
            None => return Err(format!("Missing required key in config: {}", k)),
            _ => {
                return Err(format!(
                    "Type mismatch for key {} in config: expected a {:?}, got {:?}",
                    k,
                    v.type_,
                    config.get(k)
                ));
            }
        }

        tmp_config.remove(k);
    }

    if !tmp_config.is_empty() {
        let keys = tmp_config.keys();
        return Err(format!(
            "Found unexpected keys in config: {:?}",
            keys.collect::<Vec<&String>>()
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_CONFIG: &str = r#"
description:
  name: newrelic-infra
  float_val: 0.14
  logs: -4
# overwrite the agent configuration
configuration: |
  license: abc123
  staging: true
  extra_list:
    key: value
    key2: value2
deployment:
  on_host:
    path: "/etc"
    args: --verbose true
"#;

    #[test]
    fn example_config() {
        let actual = serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG);

        assert!(actual.is_ok());
    }

    #[test]
    fn test_normalize_supervisor_config() {
        let input_structure = serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG).unwrap();
        let actual = normalize_supervisor_config(input_structure);
        let expected: NormalizedSupervisorConfig = Map::from([
            (
                "description.name".to_string(),
                TrivialValue::String("newrelic-infra".to_string()),
            ),
            (
                "description.float_val".to_string(),
                TrivialValue::Number(N::Float(0.14)),
            ),
            (
                "description.logs".to_string(),
                TrivialValue::Number(N::NegInt(-4)),
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
                "deployment.on_host.args".to_string(),
                TrivialValue::String("--verbose true".to_string()),
            ),
            (
                "deployment.on_host.path".to_string(),
                TrivialValue::String("/etc".to_string()),
            ),
        ]);

        assert_eq!(actual, expected);
    }

    const EXAMPLE_CONFIG_REPLACE: &str = r#"
deployment:
  on_host:
    path: "/etc"
    args: --verbose true
"#;
    const EXAMPLE_AGENT_YAML_REPLACE: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
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
meta:
  deployment:
    on_host:
      executables:
        - path: ${deployment.on_host.args}/otelcol
          args: "-c ${deployment.on_host.args}"
"#;

    #[test]
    fn test_validate_with_agent_type() {
        let input_structure =
            serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type = serde_yaml::from_str::<AgentType>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let expected = Map::from([
            (
                "deployment.on_host.args".to_string(),
                TrivialValue::String("--verbose true".to_string()),
            ),
            (
                "deployment.on_host.path".to_string(),
                TrivialValue::String("/etc".to_string()),
            ),
        ]);
        let actual = validate_with_agent_type(normalized, &agent_type).unwrap();

        assert_eq!(expected, actual);
    }

    const EXAMPLE_CONFIG_REPLACE_NOPATH: &str = r#"
deployment:
  on_host:
    args: --verbose true
"#;

    #[test]
    fn test_validate_with_agent_type_missing_required() {
        let input_structure =
            serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG_REPLACE_NOPATH).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type = serde_yaml::from_str::<AgentType>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let actual = validate_with_agent_type(normalized, &agent_type);

        assert!(actual.is_err());
        assert_eq!(
            actual.unwrap_err(),
            "Missing required key in config: deployment.on_host.path"
        );
    }

    const EXAMPLE_AGENT_YAML_REPLACE_WITH_DEFAULT: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  deployment:
    on_host:
      path:
        description: "Path to the agent"
        type: string
        required: false
        default: "/default_path"
      args:
        description: "Args passed to the agent"
        type: string
        required: true
meta:
  deployment:
    on_host:
      executables:
        - path: ${deployment.on_host.args}/otelcol
          args: "-c ${deployment.on_host.args}"
"#;

    #[test]
    fn test_validate_with_default() {
        let input_structure =
            serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG_REPLACE_NOPATH).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type =
            serde_yaml::from_str::<AgentType>(EXAMPLE_AGENT_YAML_REPLACE_WITH_DEFAULT).unwrap();

        let expected = Map::from([
            (
                "deployment.on_host.args".to_string(),
                TrivialValue::String("--verbose true".to_string()),
            ),
            (
                "deployment.on_host.path".to_string(),
                TrivialValue::String("/default_path".to_string()),
            ),
        ]);
        let actual = validate_with_agent_type(normalized, &agent_type).unwrap();

        assert_eq!(expected, actual);
    }
}
