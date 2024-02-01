//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Super Agent will be running.
//!
//! The reasoning behind this is that the Super Agent will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use std::{collections::HashMap, str::FromStr};

use super::agent_values::AgentValues;
use super::restart_policy::BackoffDuration;
use super::variable::definition::{VariableDefinition, VariableDefinitionTree};
use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    runtime_config::{Args, Env, Runtime},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
};
use crate::super_agent::config::AgentTypeFQN;
use crate::super_agent::defaults::default_capabilities;
use duration_str;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to start given agent binaries.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`RawAgent`]) having the spec field normalized.
///
/// See also [`RawAgent`] and the [`FinalAgent::try_from`] implementation.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct AgentType {
    pub metadata: AgentMetadata,
    pub variables: VariableTree,
    pub runtime_config: Runtime,
    capabilities: Capabilities,
}

impl AgentType {
    pub fn has_remote_management(&self) -> bool {
        self.capabilities
            .has_capability(AgentCapabilities::AcceptsRemoteConfig)
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct TemplateableValue<T> {
    value: Option<T>,
    template: String,
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // temporal type for raw deserialization
        #[derive(Debug, Deserialize)]
        struct RawAgent {
            #[serde(flatten)]
            metadata: AgentMetadata,
            variables: VariableTree,
            #[serde(default, flatten)]
            runtime_config: Runtime,
        }

        let raw_agent = RawAgent::deserialize(deserializer)?;
        Ok(Self {
            // variables: normalize_agent_spec(raw_agent.variables).map_err(D::Error::custom)?,
            variables: raw_agent.variables,
            metadata: raw_agent.metadata,
            runtime_config: raw_agent.runtime_config, // FIXME: make it actual implementation
            capabilities: default_capabilities(),
        })
    }
}

impl<'de, T> Deserialize<'de> for TemplateableValue<T> {
    fn deserialize<D>(deserializer: D) -> Result<TemplateableValue<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum InputType {
            String(String),
            NumberU64(u64),
            NumberI64(i64),
            NumberF64(f64),
            Bool(bool),
        }

        let result = match InputType::deserialize(deserializer)? {
            InputType::String(s) => s,
            InputType::NumberU64(n) => n.to_string(),
            InputType::NumberI64(n) => n.to_string(),
            InputType::NumberF64(n) => n.to_string(),
            InputType::Bool(b) => b.to_string(),
        };
        Ok(TemplateableValue {
            value: None,
            template: result,
        })
    }
}

impl<T> TemplateableValue<T> {
    pub fn get(self) -> T {
        self.value
            .unwrap_or_else(|| unreachable!("Values must be populated at this point"))
    }
    pub fn new(value: T) -> Self {
        Self {
            value: Some(value),
            template: "".to_string(),
        }
    }
    pub fn is_template_empty(&self) -> bool {
        self.template.is_empty()
    }
    #[cfg(test)]
    pub fn from_template(s: String) -> Self {
        Self {
            value: None,
            template: s,
        }
    }
    #[cfg(test)]
    pub fn with_template(self, s: String) -> Self {
        Self {
            template: s,
            ..self
        }
    }
}

impl<S> Templateable for TemplateableValue<S>
where
    S: FromStr + Default,
{
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            S::default()
        } else {
            templated_string
                .parse()
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<Env> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        Ok(Self {
            template: self.template,
            value: Some(Env(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<Args> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        Ok(Self {
            template: self.template,
            value: Some(Args(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<BackoffDuration> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            BackoffDuration::default()
        } else {
            // Attempt to parse a simple number as seconds
            duration_str::parse(&templated_string)
                .map(BackoffDuration::from)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl AgentType {
    pub fn agent_type(&self) -> AgentTypeFQN {
        self.metadata.to_string().as_str().into()
    }

    pub fn get_variables(&self) -> Variables {
        self.variables.clone().flatten()
    }

    pub fn merge_variables_with_values(
        &mut self,
        values: AgentValues,
    ) -> Result<(), AgentTypeError> {
        update_specs(values.into(), &mut self.variables.0)?;

        // No item must be left without a final value
        let not_populated = self
            .variables
            .clone()
            .flatten()
            .into_iter()
            .filter_map(|(k, endspec)| endspec.get_final_value().is_none().then_some(k))
            .collect::<Vec<_>>();

        if !not_populated.is_empty() {
            return Err(AgentTypeError::ValuesNotPopulated(not_populated));
        }
        Ok(())
    }

    /// template_with the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    pub fn template_with(
        mut self,
        values: AgentValues,
        agent_configs_path: Option<&str>,
    ) -> Result<AgentType, AgentTypeError> {
        self.merge_variables_with_values(values)?;

        let variables = if let Some(p) = agent_configs_path {
            let mut v = self.variables.clone().flatten();
            v.values_mut().for_each(|v| {
                if let Some(file_path) = v.get_file_path() {
                    let mut new_file_path = PathBuf::from(p);
                    new_file_path.push(file_path);
                    v.set_file_path(new_file_path);
                }
            });
            v
        } else {
            self.variables.clone().flatten()
        };

        let runtime_conf = self.runtime_config.template_with(&variables)?;

        let populated_agent = AgentType {
            runtime_config: runtime_conf,
            ..self
        };

        Ok(populated_agent)
    }
}

fn update_specs(
    values: HashMap<String, serde_yaml::Value>,
    agent_vars: &mut HashMap<String, VariableDefinitionTree>,
) -> Result<(), AgentTypeError> {
    for (ref k, v) in values.into_iter() {
        let spec = agent_vars
            .get_mut(k)
            .ok_or_else(|| AgentTypeError::MissingAgentKey(k.clone()))?;

        match spec {
            VariableDefinitionTree::End(e) => e.merge_with_yaml_value(v)?,
            VariableDefinitionTree::Mapping(m) => {
                let v: HashMap<String, serde_yaml::Value> = serde_yaml::from_value(v)?;
                update_specs(v, m)?
            }
        }
    }
    Ok(())
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`AgentValues`].
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct VariableTree(pub(crate) HashMap<String, VariableDefinitionTree>);

impl VariableTree {
    pub fn flatten(self) -> HashMap<String, VariableDefinition> {
        self.0
            .into_iter()
            .flat_map(|(k, v)| inner_flatten(k, v))
            .collect()
    }
}

fn inner_flatten(key: String, spec: VariableDefinitionTree) -> HashMap<String, VariableDefinition> {
    let mut result = HashMap::new();
    match spec {
        VariableDefinitionTree::End(s) => _ = result.insert(key, s),
        VariableDefinitionTree::Mapping(m) => m.into_iter().for_each(|(k, v)| {
            result.extend(inner_flatten(key.clone() + TEMPLATE_KEY_SEPARATOR + &k, v))
        }),
    }
    result
}

/// The normalized version of the [`AgentVariables`] tree.
///
/// Example of the end node in the tree:
///
/// ```yaml
/// name:
///   description: "Name of the agent"
///   type: string
///   required: false
///   default: nrdot
/// ```
///
/// The path to the end node is converted to the string with `.` as a join symbol.
///
/// ```yaml
/// variables:
///   system:
///     logging:
///       level:
///         description: "Logging level"
///         type: string
///         required: false
///         default: info
/// ```
///
/// Will be converted to `system.logging.level` and can be used later in the AgentType_Meta part as `${system.logging.level}`.
pub(crate) type Variables = HashMap<String, VariableDefinition>;

#[cfg(test)]
pub mod tests {

    use crate::agent_type::{
        restart_policy::{BackoffStrategyConfig, BackoffStrategyType, RestartPolicyConfig},
        runtime_config::Executable,
        trivial_value::{FilePathWithContent, TrivialValue},
    };

    use super::*;
    use serde_yaml::{Error, Number};
    use std::collections::HashMap as Map;

    impl AgentType {
        pub fn set_capabilities(&mut self, capabilities: Capabilities) {
            self.capabilities = capabilities
        }

        /// Retrieve the `variables` field of the agent type at the specified key, if any.
        pub fn get_variable(self, path: String) -> Option<VariableDefinition> {
            self.variables.flatten().get(&path).cloned()
        }
    }

    pub const AGENT_GIVEN_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
  description:
    name:
      description: "Name of the agent"
      type: string
      required: false
      default: nrdot
deployment:
  on_host:
    executables:
      - path: ${bin}/otelcol
        args: "-c ${deployment.k8s.image}"
        env: ""
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: 1s
            max_retries: 3
            last_retry_interval: 30s
      - path: ${bin}/otelcol-gw
        args: "-c ${deployment.k8s.image}"
        env: ""
        restart_policy:
          backoff_strategy:
            type: linear
            backoff_delay: 3s
            max_retries: 8
            last_retry_interval: 60s
"#;

    const AGENT_GIVEN_BAD_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  description:
    name:
deployment:
  on_host:
    executables:
      - path: ${bin}/otelcol
        args: "-c ${deployment.k8s.image}"
        env: ""
"#;

    // FIXME: Adapt new structure
    // #[test]
    // fn test_basic_parsing() {
    //     let agent: AgentTemplateable = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

    //     assert_eq!("nrdot", agent.metadata.name);
    //     assert_eq!("newrelic", agent.metadata.namespace);
    //     assert_eq!("0.1.0", agent.metadata.version);

    //     let on_host = agent.runtime_config.deployment.on_host.clone().unwrap();

    //     assert_eq!("${bin}/otelcol", on_host.executables[0].path);
    //     assert_eq!(
    //         Args("-c ${deployment.k8s.image}".to_string()),
    //         on_host.executables[0].args
    //     );

    //     // Restart restart policy values
    //     assert_eq!(
    //         BackoffStrategyConfigTemplateable::Fixed(BackoffStrategyInnerTemplateable {
    //             backoff_delay: Duration::from_secs(1),
    //             max_retries: 3,
    //             last_retry_interval: Duration::from_secs(30),
    //         }),
    //         on_host.restart_policy.backoff_strategy
    //     );
    // }

    #[test]
    fn test_basic_agent_parsing() {
        let agent: AgentType = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        assert_eq!("nrdot", agent.metadata.name);
        assert_eq!("newrelic", agent.metadata.namespace);
        assert_eq!("0.1.0", agent.metadata.version);

        let on_host = agent.runtime_config.deployment.on_host.clone().unwrap();

        assert_eq!(
            "${bin}/otelcol",
            on_host.executables[0].clone().path.template
        );
        assert_eq!(
            "-c ${deployment.k8s.image}".to_string(),
            on_host.executables[0].clone().args.template
        );

        // Restart policy values
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("fixed".to_string()),
                backoff_delay: TemplateableValue::from_template("1s".to_string()),
                max_retries: TemplateableValue::from_template("3".to_string()),
                last_retry_interval: TemplateableValue::from_template("30s".to_string()),
            },
            on_host.executables[0].restart_policy.backoff_strategy
        );
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("linear".to_string()),
                backoff_delay: TemplateableValue::from_template("3s".to_string()),
                max_retries: TemplateableValue::from_template("8".to_string()),
                last_retry_interval: TemplateableValue::from_template("60s".to_string()),
            },
            on_host.executables[1].restart_policy.backoff_strategy
        );
    }

    #[test]
    fn test_bad_parsing() {
        let raw_agent_err: Result<AgentType, Error> = serde_yaml::from_str(AGENT_GIVEN_BAD_YAML);

        assert!(raw_agent_err.is_err());
        println!("{:?}", raw_agent_err);
        assert_eq!(
            raw_agent_err.unwrap_err().to_string(),
            "missing field `variables` at line 2 column 1"
        );
    }

    #[test]
    fn test_normalize_agent_spec() {
        // create AgentSpec

        let given_agent: AgentType = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        let expected_map: Map<String, VariableDefinition> = Map::from([(
            "description.name".to_string(),
            VariableDefinition::new(
                "Name of the agent".to_string(),
                false,
                Some("nrdot".to_string()),
                None,
            ),
        )]);

        // expect output to be the map
        assert_eq!(expected_map, given_agent.variables.clone().flatten());

        let expected_spec = VariableDefinition::new(
            "Name of the agent".to_string(),
            false,
            Some("nrdot".to_string()),
            None,
        );

        assert_eq!(
            expected_spec,
            given_agent
                .get_variable("description.name".to_string())
                .unwrap()
        );
    }

    #[test]
    fn test_replacer() {
        let exec = Executable {
            path: TemplateableValue::from_template("${bin}/otelcol".to_string()),
            args: TemplateableValue::from_template(
                "--config ${config} --plugin_dir ${integrations} --verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: TemplateableValue::from_template("".to_string()),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template("${backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::from_template("${backoff.delay}".to_string()),
                    max_retries: TemplateableValue::from_template("${backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::from_template(
                        "${backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let normalized_values = Map::from([
            (
                "bin".to_string(),
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "config".to_string(),
                VariableDefinition::new_with_file_path(
                    "config".to_string(),
                    true,
                    None,
                    Some(FilePathWithContent::new(
                        "config2.yml".into(),
                        "license_key: abc123\nstaging: true\n".to_string(),
                    )),
                    "config_path".into(),
                ),
            ),
            (
                "integrations".to_string(),
                VariableDefinition::new_with_file_path(
                    "integrations".to_string(),
                    true,
                    None,
                    Some(HashMap::from([
                        (
                            "kafka.yml".to_string(),
                            FilePathWithContent::new(
                                "config2.yml".into(),
                                "license_key: abc123\nstaging: true\n".to_string(),
                            ),
                        ),
                        (
                            "redis.yml".to_string(),
                            FilePathWithContent::new(
                                "config2.yml".into(),
                                "license_key: abc123\nstaging: true\n".to_string(),
                            ),
                        ),
                    ])),
                    "integration_path".into(),
                ),
            ),
            (
                "deployment.on_host.verbose".to_string(),
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "deployment.on_host.log_level".to_string(),
                VariableDefinition::new(
                    "log_level".to_string(),
                    true,
                    None,
                    Some("trace".to_string()),
                ),
            ),
            (
                "backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("exponential".to_string()),
                ),
            ),
            (
                "backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "backoff.interval".to_string(),
                VariableDefinition::new(
                    "backoff_interval".to_string(),
                    true,
                    None,
                    Some("300s".to_string()),
                ),
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue {
                value: Some("/etc/otelcol".to_string()),
                template: "${bin}/otelcol".to_string(),
            },
            args: TemplateableValue {
                value: Some(Args("--config config_path --plugin_dir integration_path --verbose true --logs trace".to_string())),
                template:
                    "--config ${config} --plugin_dir ${integrations} --verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                        .to_string(),
            },
            env: TemplateableValue {
                value: Some(Env("".to_string())),
                template: "".to_string(),
            },
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Exponential),
                        template: "${backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(10)),
                        template: "${backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30),
                        template: "${backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(300)),
                        template: "${backoff.interval}".to_string(),
                    },
                },
                restart_exit_codes: vec![],
            },
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: TemplateableValue::from_template("${bin}/otelcol".to_string()),
            args: TemplateableValue::from_template("--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string()),
            env: TemplateableValue::from_template("".to_string()),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template(
                        "${backoff.type}"
                            .to_string(),
                    ),
                    backoff_delay: TemplateableValue::from_template(
                        "${backoff.delay}"
                            .to_string(),
                    ),
                    max_retries: TemplateableValue::from_template(
                        "${backoff.retries}"
                            .to_string(),
                    ),
                    last_retry_interval: TemplateableValue::from_template(
                        "${backoff.interval}"
                            .to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let normalized_values = Map::from([
            (
                "bin".to_string(),
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "deployment.on_host.verbose".to_string(),
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("linear".to_string()),
                ),
            ),
            (
                "backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "backoff.interval".to_string(),
                VariableDefinition::new(
                    "backoff_interval".to_string(),
                    true,
                    None,
                    Some("300s".to_string()),
                ),
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue { value: Some("/etc/otelcol".to_string()), template: "${bin}/otelcol".to_string() },
            args: TemplateableValue { value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string() },
            env: TemplateableValue { value: Some(Env("".to_string())), template: "".to_string() },
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Linear),
                        template: "${backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(10)),
                        template: "${backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30),
                        template: "${backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(300)),
                        template: "${backoff.interval}".to_string(),
                    },
                },
                restart_exit_codes: vec![],
            },
        };

        assert_eq!(exec_actual, exec_expected);
    }

    const GIVEN_NEWRELIC_INFRA_YAML: &str = r#"
name: newrelic-infra
namespace: newrelic
version: 1.39.1
variables:
  config:
    description: "Newrelic infra configuration yaml"
    type: file
    required: true
    file_path: "config.yml"
  config2:
    description: "Newrelic infra configuration yaml"
    type: file
    required: false
    default: |
      license_key: abc123
      staging: true
    file_path: "config2.yml"
  config3:
    description: "Newrelic infra configuration yaml"
    type: map[string]string
    required: true
  integrations:
    description: "Newrelic integrations configuration yamls"
    type: map[string]file
    required: true
    default:
      kafka: |
        bootstrap: zookeeper
    file_path: "integrations.d"
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config ${config} --config2 ${config2}"
        env: ""
"#;

    const GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML: &str = r#"
config3:
  log_level: trace
  forward: "true"
integrations:
  kafka.conf: |
    strategy: bootstrap
  redis.yml: |
    user: redis
config: |
  license_key: abc124
  staging: false
"#;

    #[test]
    fn test_template_with_runtime_field_and_agent_configs_path() {
        // Having Agent Type
        let input_agent_type =
            serde_yaml::from_str::<AgentType>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();

        // And Agent Values
        let input_user_config =
            serde_yaml::from_str::<AgentValues>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML).unwrap();

        // When populating values
        let actual = input_agent_type
            .template_with(input_user_config, Some("an/agents-config/path"))
            .expect("Failed to template_with the AgentType's runtime_config field");

        // Then we expected final values
        // MapStringString
        let expected_config_3: TrivialValue = HashMap::from([
            ("log_level".to_string(), "trace".to_string()),
            ("forward".to_string(), "true".to_string()),
        ])
        .into();
        // File with default
        let expected_config_2: TrivialValue = FilePathWithContent::new(
            "config2.yml".into(),
            "license_key: abc123\nstaging: true\n".to_string(),
        )
        .into();
        // File with values
        let expected_config: TrivialValue = FilePathWithContent::new(
            "config.yml".into(),
            "license_key: abc124\nstaging: false\n".to_string(),
        )
        .into();
        // MapStringFile
        let expected_integrations: TrivialValue = HashMap::from([
            (
                "kafka.conf".to_string(),
                FilePathWithContent::new(
                    "integrations.d".into(),
                    "strategy: bootstrap\n".to_string(),
                ),
            ),
            (
                "redis.yml".to_string(),
                FilePathWithContent::new("integrations.d".into(), "user: redis\n".to_string()),
            ),
        ])
        .into();

        let expected_executable_args_with_abs_pat =
            "--config an/agents-config/path/config.yml --config2 an/agents-config/path/config2.yml";

        assert_eq!(
            expected_config_3,
            actual
                .get_variables()
                .get("config3")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_config_2,
            actual
                .get_variables()
                .get("config2")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_config,
            actual
                .get_variables()
                .get("config")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_integrations,
            actual
                .get_variables()
                .get("integrations")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_executable_args_with_abs_pat,
            actual
                .runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables[0]
                .args
                .value
                .clone()
                .unwrap()
                .0
        );
    }

    const AGENT_BACKOFF_TEMPLATE_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
  backoff:
    delay:
      description: "Backoff delay"
      type: string
      required: false
      default: 1s
    retries:
      description: "Backoff retries"
      type: number
      required: false
      default: 3
    interval:
      description: "Backoff interval"
      type: string
      required: false
      default: 30s
    type:
      description: "Backoff strategy type"
      type: string
      required: true
deployment:
  on_host:
    executables:
      - path: /bin/otelcol
        args: "-c some-arg"
        env: ""
        restart_policy:
          backoff_strategy:
            type: ${backoff.type}
            backoff_delay: ${backoff.delay}
            max_retries: ${backoff.retries}
            last_retry_interval: ${backoff.interval}
"#;

    const BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10s
  retries: 30
  interval: 300s
  type: linear
"#;

    #[test]
    fn test_backoff_config() {
        let input_agent_type =
            serde_yaml::from_str::<AgentType>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();
        // println!("Input: {:#?}", input_agent_type);

        let input_user_config = serde_yaml::from_str::<AgentValues>(BACKOFF_CONFIG_YAML).unwrap();
        // println!("Input: {:#?}", input_user_config);

        let expected_backoff = BackoffStrategyConfig {
            backoff_type: TemplateableValue {
                value: Some(BackoffStrategyType::Linear),
                template: "${backoff.type}".to_string(),
            },
            backoff_delay: TemplateableValue {
                value: Some(BackoffDuration::from_secs(10)),
                template: "${backoff.delay}".to_string(),
            },
            max_retries: TemplateableValue {
                value: Some(30),
                template: "${backoff.retries}".to_string(),
            },
            last_retry_interval: TemplateableValue {
                value: Some(BackoffDuration::from_secs(300)),
                template: "${backoff.interval}".to_string(),
            },
        };

        let actual = input_agent_type
            .template_with(input_user_config, None)
            .expect("Failed to template_with the AgentType's runtime_config field");

        // println!("Output: {:#?}", actual);
        assert_eq!(
            expected_backoff,
            actual
                .runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables[0]
                .restart_policy
                .backoff_strategy
        );
    }

    const WRONG_RETRIES_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: -30
  interval: 300
  type: linear
"#;

    const WRONG_DELAY_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: -10
  retries: 30
  interval: 300
  type: linear
"#;
    const WRONG_INTERVAL_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: 30
  interval: -300
  type: linear
"#;

    const WRONG_TYPE_BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: 30
  interval: -300
  type: fafafa
"#;

    #[test]
    fn test_negative_backoff_configs() {
        let input_agent_type =
            serde_yaml::from_str::<AgentType>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();

        let wrong_retries =
            serde_yaml::from_str::<AgentValues>(WRONG_RETRIES_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_delay =
            serde_yaml::from_str::<AgentValues>(WRONG_DELAY_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_interval =
            serde_yaml::from_str::<AgentValues>(WRONG_INTERVAL_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_type =
            serde_yaml::from_str::<AgentValues>(WRONG_TYPE_BACKOFF_CONFIG_YAML).unwrap();

        let actual = input_agent_type.clone().template_with(wrong_retries, None);
        assert!(actual.is_err());

        let actual = input_agent_type.clone().template_with(wrong_delay, None);
        assert!(actual.is_err());

        let actual = input_agent_type.clone().template_with(wrong_interval, None);
        assert!(actual.is_err());

        let actual = input_agent_type.template_with(wrong_type, None);
        assert!(actual.is_err());
    }

    const AGENT_STRING_DURATIONS_TEMPLATE_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
variables:
  backoff:
    delay:
      description: "Backoff delay"
      type: string
      required: false
      default: 1s
    retries:
      description: "Backoff retries"
      type: number
      required: false
      default: 3
    interval:
      description: "Backoff interval"
      type: string
      required: false
      default: 30s
    type:
      description: "Backoff type"
      type: string
      required: true
deployment:
  on_host:
    executables:
      - path: /bin/otelcol
        args: "-c some-arg"
        env: ""
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: ${backoff.delay}
            max_retries: ${backoff.retries}
            last_retry_interval: ${backoff.interval}
"#;

    const STRING_DURATIONS_CONFIG_YAML: &str = r#"
backoff:
  delay: 10m + 30s
  retries: 30
  interval: 5m
  type: fixed
"#;

    #[test]
    fn test_string_backoff_config() {
        let input_agent_type =
            serde_yaml::from_str::<AgentType>(AGENT_STRING_DURATIONS_TEMPLATE_YAML).unwrap();

        let input_user_config =
            serde_yaml::from_str::<AgentValues>(STRING_DURATIONS_CONFIG_YAML).unwrap();

        let expected_backoff = BackoffStrategyConfig {
            backoff_type: TemplateableValue {
                value: Some(BackoffStrategyType::Fixed),
                template: "fixed".to_string(),
            },
            backoff_delay: TemplateableValue {
                value: Some(BackoffDuration::from_secs((10 * 60) + 30)),
                template: "${backoff.delay}".to_string(),
            },
            max_retries: TemplateableValue {
                value: Some(30),
                template: "${backoff.retries}".to_string(),
            },
            last_retry_interval: TemplateableValue {
                value: Some(BackoffDuration::from_secs(300)),
                template: "${backoff.interval}".to_string(),
            },
        };

        let actual = input_agent_type
            .template_with(input_user_config, None)
            .expect("Failed to template_with the AgentType's runtime_config field");

        // println!("Output: {:#?}", actual);
        assert_eq!(
            expected_backoff,
            actual
                .runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables[0]
                .restart_policy
                .backoff_strategy
        );
    }

    const K8S_AGENT_TYPE_YAML_VARIABLES: &str = r#"
name: k8s-agent-type
namespace: newrelic
version: 0.0.1
variables:
  config:
    values:
      description: "yaml values"
      type: yaml
      required: true
deployment:
  k8s:
    objects:
      cr1:
        apiVersion: group/version
        kind: ObjectKind
        spec:
          values: ${config.values}
"#;

    const K8S_CONFIG_YAML_VALUES: &str = r#"
config:
  values:
    key: value
    another_key:
      nested: nested_value
      nested_list:
        - item1
        - item2
        - item3_nested: value
    empty_key:
"#;

    #[test]
    fn test_k8s_config_yaml_variables() {
        let input_agent_type: AgentType =
            serde_yaml::from_str(K8S_AGENT_TYPE_YAML_VARIABLES).unwrap();
        let user_config: AgentValues = serde_yaml::from_str(K8S_CONFIG_YAML_VALUES).unwrap();
        let expected_spec_yaml = r#"
values:
  key: value
  another_key:
    nested: nested_value
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
"#;
        let expected_spec_value: serde_yaml::Value =
            serde_yaml::from_str(expected_spec_yaml).unwrap();

        let expanded_final_agent = input_agent_type.template_with(user_config, None).unwrap();

        let cr1 = expanded_final_agent
            .runtime_config
            .deployment
            .k8s
            .unwrap()
            .objects
            .get("cr1")
            .unwrap()
            .clone();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    const AGENT_WITH_VARIANTS: &str = r#"
name: variant-values
namespace: newrelic
version: 0.0.1
variables:
  restart_policy:
    type:
      description: "restart policy type"
      type: string
      required: false
      variants: [fixed, linear]
      default: exponential
deployment:
  on_host:
      executables:
      - path: /bin/echo
        args: "${restart_policy.type}"
"#;

    const CONFIG_YAML_VALUES_VALID_VARIANT: &str = r#"
restart_policy:
    type: fixed
"#;

    const CONFIG_YAML_VALUES_INVALID_VARIANT: &str = r#"
restart_policy:
    type: random
"#;

    #[test]
    fn test_agent_with_variants() {
        let input_agent_type: AgentType = serde_yaml::from_str(AGENT_WITH_VARIANTS).unwrap();
        let user_config: AgentValues = serde_yaml::from_str(CONFIG_YAML_VALUES_VALID_VARIANT)
            .expect("Failed to parse user config");
        let expanded_final_agent = input_agent_type.template_with(user_config, None).unwrap();

        let executable = expanded_final_agent
            .runtime_config
            .deployment
            .on_host
            .unwrap();

        let actual_exec = executable.executables.first().unwrap();

        assert_eq!(actual_exec.path.value, Some("/bin/echo".to_string()));
        assert_eq!(actual_exec.args.value, Some(Args("fixed".to_string())));
    }

    #[test]
    fn test_agent_with_variants_invalid() {
        let input_agent_type: AgentType = serde_yaml::from_str(AGENT_WITH_VARIANTS).unwrap();
        let user_config: AgentValues = serde_yaml::from_str(CONFIG_YAML_VALUES_INVALID_VARIANT)
            .expect("Failed to parse user config");
        let expanded_final_agent = input_agent_type.template_with(user_config, None);

        assert!(expanded_final_agent.is_err());
        assert_eq!(
            expanded_final_agent.unwrap_err().to_string(),
            r#"Invalid variant provided as a value: `"random"`. Variants allowed: ["\"fixed\"", "\"linear\""]"#
        );
    }

    #[test]
    fn default_can_be_invalid_variant() {
        let input_agent_type: AgentType = serde_yaml::from_str(AGENT_WITH_VARIANTS).unwrap();
        let user_config = AgentValues::default();
        let expanded_final_agent = input_agent_type.template_with(user_config, None);

        assert!(expanded_final_agent.is_ok());

        let executable = expanded_final_agent
            .unwrap()
            .runtime_config
            .deployment
            .on_host
            .unwrap();

        let actual_exec = executable.executables.first().unwrap();

        assert_eq!(actual_exec.path.value, Some("/bin/echo".to_string()));
        assert_eq!(
            actual_exec.args.value,
            Some(Args("exponential".to_string()))
        );
    }
}
