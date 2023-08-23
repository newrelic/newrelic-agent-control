use serde::Deserialize;
use std::collections::HashMap;

use crate::config::agent_type::SpecType;

use super::agent_type::AgentType;

type SupervisorConfig = HashMap<String, SupervisorConfigInner>;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum SupervisorConfigInner {
    NestedConfig(HashMap<String, SupervisorConfigInner>),
    EndValue(TrivialValue),
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum TrivialValue {
    String(String),
    Bool(bool),
    Number(N),
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum N {
    PosInt(u64),
    /// Always less than zero.
    NegInt(i64),
    /// May be infinite or NaN.
    Float(f64),
}

type NormalizedSupervisorConfig = HashMap<String, TrivialValue>;

fn normalize_supervisor_config(config: SupervisorConfig) -> NormalizedSupervisorConfig {
    let mut result = HashMap::new();
    config
        .into_iter()
        .for_each(|(k, v)| result.extend(inner_normalize(k, v)));
    result
}

fn inner_normalize(key: String, config: SupervisorConfigInner) -> NormalizedSupervisorConfig {
    let mut result = HashMap::new();
    match config {
        SupervisorConfigInner::NestedConfig(c) => c
            .into_iter()
            .for_each(|(k, v)| result.extend(inner_normalize(key.clone() + "/" + &k, v))),
        SupervisorConfigInner::EndValue(v) => _ = result.insert(key, v),
    }
    result
}

fn validate_with_agent_type(
    config: NormalizedSupervisorConfig,
    agent_type: &AgentType,
) -> Result<(), String> {
    // What do we need to do?
    // Check that all the keys in the agent_type are present in the config
    // Also, check that all the values of the config are of the type declared in the config's NormalizedSpec

    for (k, v) in agent_type.spec.iter() {
        if !config.contains_key(k) && v.required {
            return Err(format!("Missing required key in config: {}", k));
        }
        // Check if the types match
        match config.get(k) {
            Some(TrivialValue::String(_)) if v.type_ == SpecType::String => {}
            Some(TrivialValue::Bool(_)) if v.type_ == SpecType::Bool => {}
            Some(TrivialValue::Number(_)) if v.type_ == SpecType::Number => {}
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
    }
    Ok(())
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
    args: --verbose true
"#;
    const EXAMPLE_AGENT_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  description:
    name:
      description: "Name of the agent"
      type: string
      required: false
      default: nrdot
meta:
  deployment:
    on_host:
      executables:
        - path: ${bin}/otelcol
          args: "-c ${deployment.k8s.image}"
"#;

    #[test]
    fn example_config() {
        let actual = serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG);

        println!("{:#?}", actual);
        assert!(actual.is_ok());
        // assert_eq!(actual.unwrap(), )
    }

    #[test]
    fn test_normalize_supervisor_config() {
        let input_structure = serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG).unwrap();
        let actual = normalize_supervisor_config(input_structure);
        let expected: NormalizedSupervisorConfig = HashMap::from([
            (
                "description/name".to_string(),
                TrivialValue::String("newrelic-infra".to_string()),
            ),
            (
                "description/float_val".to_string(),
                TrivialValue::Number(N::Float(0.14)),
            ),
            (
                "description/logs".to_string(),
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
                "deployment/on_host/args".to_string(),
                TrivialValue::String("--verbose true".to_string()),
            ),
        ]);

        println!("{:#?}", expected);
        println!("{:#?}", actual);

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_validate_with_agent_type() {
        let input_structure = serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type = serde_yaml::from_str::<AgentType>(EXAMPLE_AGENT_YAML).unwrap();

        let actual = validate_with_agent_type(normalized, &agent_type);

        println!("{:#?}", agent_type);

        assert!(actual.is_ok());
    }
}
