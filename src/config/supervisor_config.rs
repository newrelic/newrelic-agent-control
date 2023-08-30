use serde::Deserialize;
use std::collections::HashMap as Map;
use std::fs;
use std::io::Write;
use tracing::error;

use uuid::Uuid;

use super::agent_type::{Agent, AgentTypeError, TrivialValue, TEMPLATE_KEY_SEPARATOR};

#[derive(Debug, PartialEq, Deserialize)]
pub struct SupervisorConfig(Map<String, SupervisorConfigInner>);

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum SupervisorConfigInner {
    NestedConfig(Map<String, SupervisorConfigInner>),
    EndValue(TrivialValue),
}

pub type NormalizedSupervisorConfig = Map<String, TrivialValue>;

impl From<SupervisorConfig> for NormalizedSupervisorConfig {
    fn from(config: SupervisorConfig) -> Self {
        normalize_supervisor_config(config)
    }
}

fn normalize_supervisor_config(config: SupervisorConfig) -> NormalizedSupervisorConfig {
    config.0.into_iter().fold(Map::new(), |r, (k, v)| {
        r.into_iter().chain(inner_normalize(k, v)).collect()
    })
}

fn inner_normalize(key: String, config: SupervisorConfigInner) -> NormalizedSupervisorConfig {
    match config {
        SupervisorConfigInner::NestedConfig(c) => c.into_iter().fold(Map::new(), |r, (k, v)| {
            r.into_iter()
                .chain(inner_normalize(
                    key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                    v,
                ))
                .collect()
        }),
        SupervisorConfigInner::EndValue(v) => Map::from([(key, v)]),
    }
}

pub fn validate_with_agent_type(
    config: NormalizedSupervisorConfig,
    agent_type: &Agent,
) -> Result<NormalizedSupervisorConfig, AgentTypeError> {
    // What do we need to do?
    // Check that all the keys in the agent_type are present in the config
    // Also, check that all the values of the config are of the type declared in the config's NormalizedSpec
    let mut result = Map::new();
    let mut tmp_config = config.clone();

    for (k, v) in agent_type.variables.iter() {
        if !tmp_config.contains_key(k) && v.required {
            return Err(AgentTypeError::MissingAgentKey(k.clone()));
        }

        if !tmp_config.contains_key(k) {
            // We have validated earlier that for a not `required` value, a default will be provided
            // so we could just unwrap. Panicking with a certain message here to catch a potential edge case.
            result.insert(
                k.clone(),
                v.default
                    .clone()
                    .expect("Failed to retrieve default for a non-required value"),
            );
            continue;
        }

        // Get the key and its value
        tmp_config
            .get(k)
            .map(|tv| tv.clone().check_type(v.type_))
            .transpose()?
            .map(|tv| _ = result.insert(k.clone(), tv))
            .ok_or(AgentTypeError::MissingAgentKey(k.clone()))?;

        tmp_config.remove(k);
    }

    if !tmp_config.is_empty() {
        return Err(AgentTypeError::UnexpectedKeysInConfig(
            tmp_config.into_keys().collect::<Vec<String>>(),
        ));
    }

    write_files(&mut result)?;

    Ok(result)
}

fn write_files(config: &mut NormalizedSupervisorConfig) -> Result<(), AgentTypeError> {
    error!("Reached write_files");
    config
        .values_mut()
        .try_for_each(|v| -> Result<(), AgentTypeError> {
            if let TrivialValue::File(f) = v {
                const CONF_DIR: &str = "agentconfigs";
                // get current path
                let wd = std::env::current_dir()?;
                let dir = wd.join(CONF_DIR);
                if !dir.exists() {
                    fs::create_dir(dir.as_path())?;
                }
                let uuid = Uuid::new_v4().to_string();
                let path = format!("{}/{}-config.yaml", dir.to_string_lossy(), uuid); // FIXME: PATH?
                error!("path: {}", path);
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&path)?;

                writeln!(file, "{}", f.content)?;
                f.path = path;
                // f.path = file
                //     .path()
                //     .to_str()
                //     .ok_or(AgentTypeError::InvalidFilePath)?
                //     .to_string();
            }
            Ok(())
        })
}

#[cfg(test)]
mod tests {
    use crate::config::agent_type::N;

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
    env: ""
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
            (
                "deployment.on_host.env".to_string(),
                TrivialValue::String("".to_string()),
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
variables:
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
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.path}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;

    #[test]
    fn test_validate_with_agent_type() {
        let input_structure =
            serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type = serde_yaml::from_str::<Agent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

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
        let agent_type = serde_yaml::from_str::<Agent>(EXAMPLE_AGENT_YAML_REPLACE).unwrap();

        let actual = validate_with_agent_type(normalized, &agent_type);

        assert!(actual.is_err());
        assert_eq!(
            format!("{}", actual.unwrap_err()),
            "Missing required key in config: `deployment.on_host.path`"
        );
    }

    const EXAMPLE_AGENT_YAML_REPLACE_WITH_DEFAULT: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
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
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.args}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;

    #[test]
    fn test_validate_with_default() {
        let input_structure =
            serde_yaml::from_str::<SupervisorConfig>(EXAMPLE_CONFIG_REPLACE_NOPATH).unwrap();
        let normalized = normalize_supervisor_config(input_structure);
        let agent_type =
            serde_yaml::from_str::<Agent>(EXAMPLE_AGENT_YAML_REPLACE_WITH_DEFAULT).unwrap();

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
