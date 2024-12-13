use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config_templates::Templateable;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// The YAMLConfig represent any YAML config that the AgentControl can read and store.
/// It enforces that the root of the tree is a hashmap and not an array or a single element.
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct YAMLConfig(HashMap<String, serde_yaml::Value>);

#[derive(Error, Debug)]
pub enum YAMLConfigError {
    #[error("invalid agent values format: `{0}`")]
    FormatError(#[from] serde_yaml::Error),
}

impl Templateable for YAMLConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self(self.0.template_with(variables)?))
    }
}

impl Templateable for HashMap<String, serde_yaml::Value> {
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

impl TryFrom<String> for YAMLConfig {
    type Error = YAMLConfigError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(serde_yaml::from_str::<YAMLConfig>(value.as_str())?)
    }
}

impl TryFrom<YAMLConfig> for String {
    type Error = YAMLConfigError;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        //serde_yaml::to_string returns "{}\n" if value is empty
        if value.0.is_empty() {
            return Ok("".to_string());
        }
        Ok(serde_yaml::to_string(&value)?)
    }
}

pub fn has_remote_management(capabilities: &Capabilities) -> bool {
    capabilities.has_capability(AgentCapabilities::AcceptsRemoteConfig)
}

#[cfg(test)]
mod tests {

    use serde_yaml::{Mapping, Value};

    use crate::agent_type::{
        definition::AgentType,
        environment::Environment,
        trivial_value::FilePathWithContent,
        variable::definition::{VariableDefinition, VariableDefinitionTree},
    };

    use super::*;

    impl YAMLConfig {
        pub(crate) fn new(values: HashMap<String, Value>) -> Self {
            Self(values)
        }

        #[allow(dead_code)]
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
deployment:
  on_host:
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
    executable:
      path: ${deployment.on_host.path}/otelcol
      args: "-c ${deployment.on_host.args}"
"#;

    #[test]
    fn test_update_specs() {
        let input_structure = serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE).unwrap();
        let agent_type =
            AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE, &Environment::OnHost);

        let expected = HashMap::from([
            (
                "deployment".to_string(),
                VariableDefinitionTree::Mapping(HashMap::from([(
                    "on_host".to_string(),
                    VariableDefinitionTree::Mapping(HashMap::from([
                        (
                            "path".to_string(),
                            VariableDefinitionTree::End(VariableDefinition::new(
                                "Path to the agent".to_string(),
                                true,
                                None,
                                Some("/etc".to_string()),
                            )),
                        ),
                        (
                            "args".to_string(),
                            VariableDefinitionTree::End(VariableDefinition::new(
                                "Args passed to the agent".to_string(),
                                true,
                                None,
                                Some("--verbose true".to_string()),
                            )),
                        ),
                    ])),
                )])),
            ),
            (
                "config".to_string(),
                VariableDefinitionTree::End(VariableDefinition::new_with_file_path(
                    "Path to the agent".to_string(),
                    true,
                    None,
                    Some(FilePathWithContent::new(
                        "newrelic-infra.yml".into(),
                        "test\n".to_string(),
                    )),
                    "newrelic-infra.yml".into(),
                )),
            ),
            (
                "integrations".to_string(),
                VariableDefinitionTree::End(VariableDefinition::new_with_file_path(
                    "Newrelic integrations configuration yamls".to_string(),
                    true,
                    None,
                    Some(HashMap::from([(
                        "kafka".into(),
                        FilePathWithContent::new(
                            "integrations.d".into(),
                            "strategy: bootstrap\n".to_string(),
                        ),
                    )])),
                    "integrations.d".into(),
                )),
            ),
        ]);

        let filled_variables = agent_type
            .variables
            .fill_with_values(input_structure)
            .unwrap();

        assert_eq!(expected, filled_variables.0);
    }

    const EXAMPLE_CONFIG_REPLACE_WRONG_TYPE: &str = r#"
    config: |
      test
    deployment:
      on_host:
        path: true
        args: --verbose true
    integrations: {}
    "#;

    #[test]
    fn test_validate_with_agent_type_wrong_value_type() {
        let input_structure =
            serde_yaml::from_str::<YAMLConfig>(EXAMPLE_CONFIG_REPLACE_WRONG_TYPE).unwrap();
        let agent_type =
            AgentType::build_for_testing(EXAMPLE_AGENT_YAML_REPLACE, &Environment::OnHost);

        let result = agent_type.variables.fill_with_values(input_structure);

        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Error while parsing: `invalid type: boolean `true`, expected a string`"
        );
    }
}
