use serde::Deserialize;
use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};
use tempfile::NamedTempFile;

use crate::config::agent_type::SpecType;
use std::io::Write;

use super::agent_type::{AgentType, AgentTypeError, TEMPLATE_KEY_SEPARATOR};

pub(crate) type SupervisorConfig = Map<String, SupervisorConfigInner>;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum SupervisorConfigInner {
    NestedConfig(Map<String, SupervisorConfigInner>),
    EndValue(TrivialValue),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    Bool(bool),
    Number(N),
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{}", s),
            TrivialValue::File(file) => write!(f, "{}", file.path),
            TrivialValue::Bool(b) => write!(f, "{}", b),
            TrivialValue::Number(n) => write!(f, "{}", n),
        }
    }
}

#[derive(Debug, PartialEq, Default, Clone)]
pub(crate) struct FilePathWithContent {
    path: String,
    content: String,
}

impl FilePathWithContent {
    pub(crate) fn new(content: String) -> Self {
        FilePathWithContent {
            content,
            ..Default::default()
        }
    }
}

impl<'de> Deserialize<'de> for FilePathWithContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(FilePathWithContent::new(String::deserialize(deserializer)?))
    }
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

impl Display for N {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            N::PosInt(n) => write!(f, "{}", n),
            N::NegInt(n) => write!(f, "{}", n),
            N::Float(n) => write!(f, "{}", n),
        }
    }
}

pub(crate) type NormalizedSupervisorConfig = Map<String, TrivialValue>;

pub(crate) fn normalize_supervisor_config(config: SupervisorConfig) -> NormalizedSupervisorConfig {
    let mut result = Map::new();
    config
        .into_iter()
        .for_each(|(k, v)| result.extend(inner_normalize(k, v)));
    result
}

fn inner_normalize(key: String, config: SupervisorConfigInner) -> NormalizedSupervisorConfig {
    let mut result = Map::new();
    match config {
        SupervisorConfigInner::NestedConfig(c) => c.into_iter().for_each(|(k, v)| {
            result.extend(inner_normalize(
                key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                v,
            ))
        }),
        SupervisorConfigInner::EndValue(v) => _ = result.insert(key, v),
    }
    result
}

pub(crate) fn validate_with_agent_type(
    config: NormalizedSupervisorConfig,
    agent_type: &AgentType,
) -> Result<NormalizedSupervisorConfig, AgentTypeError> {
    // What do we need to do?
    // Check that all the keys in the agent_type are present in the config
    // Also, check that all the values of the config are of the type declared in the config's NormalizedSpec
    let mut result: NormalizedSupervisorConfig = Map::new();
    let mut tmp_config = config.clone();

    for (k, v) in agent_type.spec.iter() {
        if !tmp_config.contains_key(k) && v.required {
            return Err(AgentTypeError::MissingAgentKey(k.clone()));
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
            Some(TrivialValue::String(s)) if v.type_ == SpecType::File => {
                let f = TrivialValue::File(FilePathWithContent::new(s.clone()));
                _ = result.insert(k.clone(), f)
            }
            Some(b @ TrivialValue::Bool(_)) if v.type_ == SpecType::Bool => {
                _ = result.insert(k.clone(), b.clone())
            }
            Some(n @ TrivialValue::Number(_)) if v.type_ == SpecType::Number => {
                _ = result.insert(k.clone(), n.clone())
            }
            None => return Err(AgentTypeError::MissingAgentKey(k.clone())),
            _ => {
                return Err(AgentTypeError::MismatchedTypes {
                    key: k.clone(),
                    expected_type: v.type_.clone(),
                    actual_value: config.get(k).unwrap().clone(),
                });
            }
        }

        tmp_config.remove(k);
    }

    if !tmp_config.is_empty() {
        let keys = tmp_config.keys();
        return Err(AgentTypeError::UnexpectedKeysInConfig(
            keys.cloned().collect::<Vec<String>>(),
        ));
    }

    for (k, v) in result.clone() {
        if let TrivialValue::File(f) = v {
            let content = f.content;

            // FIXME: What happens when early removal of the temp file while the SuperAgent is still running?
            let mut file = NamedTempFile::new()?;
            writeln!(file, "{content}")?;
            let file_path = file.path();

            let final_file = TrivialValue::File(FilePathWithContent {
                path: file_path
                    .to_str()
                    .ok_or(AgentTypeError::InvalidFilePath)?
                    .to_string(),
                content,
            });

            result.insert(k, final_file);
        }
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
        - path: ${deployment.on_host.path}/otelcol
          args: "-c ${deployment.on_host.args}"
          env: ""
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
            format!("{}", actual.unwrap_err()),
            "Missing required key in config: `deployment.on_host.path`"
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
          env: ""
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
