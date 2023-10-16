//! This module contains the definitions of the Supervisor's Agent Type, which is the type of agent that the Supervisor will be running.
//!
//! The reasoning behind this is that the Supervisor will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use std::{collections::HashMap, str::FromStr};

use serde::{Deserialize, Deserializer};

use super::restart_policy::BackoffDuration;
use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    runtime_config::{Args, Env, RuntimeConfig},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
    trivial_value::TrivialValue,
};
use crate::config::supervisor_config::SupervisorConfig;

#[derive(Debug, Deserialize)]
struct RawAgent {
    #[serde(flatten)]
    metadata: AgentMetadata,
    variables: AgentVariables,
    #[serde(default, flatten)]
    runtime_config: RuntimeConfig,
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct TemplateableValue<T> {
    value: Option<T>,
    template: String,
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
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
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
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        Ok(Self {
            template: self.template,
            value: Some(Env(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<Args> {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        Ok(Self {
            template: self.template,
            value: Some(Args(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<BackoffDuration> {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            BackoffDuration::default()
        } else {
            BackoffDuration::from_secs(
                templated_string
                    .parse()
                    .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?,
            )
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to start given agent binaries.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`RawAgent`]) having the spec field normalized.
///
/// See also [`RawAgent`] and the [`FinalAgent::try_from`] implementation.
#[derive(Debug, PartialEq, Clone, Default, Deserialize)]
#[serde(try_from = "RawAgent")]
pub struct FinalAgent {
    #[serde(flatten)]
    pub metadata: AgentMetadata,
    pub variables: NormalizedVariables,
    pub runtime_config: RuntimeConfig,
}

impl FinalAgent {
    pub fn get_variables(&self) -> &NormalizedVariables {
        &self.variables
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// template_with the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    pub fn template_with(self, config: SupervisorConfig) -> Result<FinalAgent, AgentTypeError> {
        // let normalized_config = NormalizedSupervisorConfig::from(config);
        // let validated_conf = validate_with_agent_type(normalized_config, &self)?;
        let config = config.normalize_with_agent_type(&self)?;

        // let runtime_conf = self.runtime_config.template_with(validated_conf.clone())?;
        let mut spec = self.variables;

        // modifies variables final value with the one defined in the SupervisorConfig
        spec.iter_mut().for_each(|(k, v)| {
            let defined_value = config.get_from_normalized(k);
            v.final_value = defined_value.or(v.default.clone());
        });

        let runtime_conf = self.runtime_config.template_with(&spec)?;

        let populated_agent = FinalAgent {
            runtime_config: runtime_conf,
            variables: spec,
            ..self
        };

        Ok(populated_agent)
    }
}

impl TryFrom<RawAgent> for FinalAgent {
    type Error = AgentTypeError;
    /// Convert a [`RawAgent`] into an [`Agent`].
    ///
    /// This is where the `variables` field of the [`RawAgent`] is normalized into a [`NormalizedVariables`].
    fn try_from(raw_agent: RawAgent) -> Result<Self, Self::Error> {
        Ok(Self {
            variables: normalize_agent_spec(raw_agent.variables)?,
            metadata: raw_agent.metadata,
            runtime_config: raw_agent.runtime_config, // FIXME: make it actual implementation
        })
    }
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`SupervisorConfig`].
type AgentVariables = HashMap<String, Spec>;

pub trait AgentTypeEndSpec {
    fn variable_type(&self) -> VariableType;
    fn file_path(&self) -> Option<String>;
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(try_from = "IntermediateEndSpec")]
pub struct EndSpec {
    pub(crate) description: String,
    #[serde(rename = "type")]
    pub type_: VariableType,
    pub required: bool,
    pub default: Option<TrivialValue>,
    pub file_path: Option<String>,
    /// The actual value that will be used by the agent. This will be either the user-provided value or, if not provided and not marked as [`required`], the default value.
    #[serde(skip)]
    pub final_value: Option<TrivialValue>,
}

impl AgentTypeEndSpec for EndSpec {
    fn variable_type(&self) -> VariableType {
        self.type_
    }

    fn file_path(&self) -> Option<String> {
        self.file_path.as_ref().cloned()
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Deserialize)]
pub enum VariableType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "map[string]string")]
    MapStringString,
    #[serde(rename = "map[string]file")]
    MapStringFile,
    // #[serde(rename = "map[string]number")]
    // MapStringNumber,
    // #[serde(rename = "map[string]bool")]
    // MapStringBool,
}

#[derive(Debug, Deserialize)]
struct IntermediateEndSpec {
    description: String,
    #[serde(rename = "type")]
    type_: VariableType,
    required: bool,
    default: Option<TrivialValue>,
    file_path: Option<String>,
}

impl AgentTypeEndSpec for IntermediateEndSpec {
    fn variable_type(&self) -> VariableType {
        self.type_
    }

    fn file_path(&self) -> Option<String> {
        self.file_path.as_ref().cloned()
    }
}

impl TryFrom<IntermediateEndSpec> for EndSpec {
    type Error = AgentTypeError;

    /// Convert a [`IntermediateEndSpec`] into an [`EndSpec`].
    ///
    /// This conversion will fail if there is no default value and the spec is not marked as [`required`], as there will be no value to use. Also, the type for the provided default value will be checked against the [`VariableType`], failing if it does not match.
    fn try_from(ies: IntermediateEndSpec) -> Result<Self, Self::Error> {
        if ies.default.is_none() && !ies.required {
            return Err(AgentTypeError::MissingDefault);
        }
        let def_val = ies
            .default
            .clone()
            .map(|d| d.check_type(&ies))
            .transpose()?;
        Ok(EndSpec {
            default: def_val,
            final_value: None,
            file_path: ies.file_path,
            description: ies.description,
            type_: ies.type_,
            required: ies.required,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct K8s {
    crd: String,
}

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum Spec {
    SpecEnd(EndSpec),
    SpecMapping(HashMap<String, Spec>),
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
pub(crate) type NormalizedVariables = HashMap<String, EndSpec>;

fn normalize_agent_spec(spec: AgentVariables) -> Result<NormalizedVariables, AgentTypeError> {
    spec.into_iter().try_fold(HashMap::new(), |r, (k, v)| {
        let n_spec = inner_normalize(k, v);
        n_spec.iter().try_for_each(|(k, end_spec)| {
            if end_spec.default.is_none() && !end_spec.required {
                return Err(AgentTypeError::MissingDefaultWithKey(k.clone()));
            }
            Ok(())
        })?;
        Ok(r.into_iter().chain(n_spec).collect())
    })
}

fn inner_normalize(key: String, spec: Spec) -> NormalizedVariables {
    let mut result = HashMap::new();
    match spec {
        Spec::SpecEnd(s) => _ = result.insert(key, s),
        Spec::SpecMapping(m) => m.into_iter().for_each(|(k, v)| {
            result.extend(inner_normalize(
                key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                v,
            ))
        }),
    }
    result
}

#[cfg(test)]
impl FinalAgent {
    /// Retrieve the `variables` field of the agent type at the specified key, if any.
    fn get_variable(self, path: String) -> Option<EndSpec> {
        self.variables.get(&path).cloned()
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::{
        agent_type::{
            restart_policy::{BackoffStrategyConfig, BackoffStrategyType},
            runtime_config::{Args, Env, Executable},
        },
        supervisor_config::SupervisorConfig,
    };

    use super::*;
    use crate::config::agent_type::restart_policy::RestartPolicyConfig;
    use crate::config::agent_type::trivial_value::FilePathWithContent;
    use crate::config::agent_type::trivial_value::N::PosInt;
    use serde_yaml::Error;
    use std::collections::HashMap as Map;

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
            backoff_delay_seconds: 1
            max_retries: 3
            last_retry_interval_seconds: 30
      - path: ${bin}/otelcol-gw
        args: "-c ${deployment.k8s.image}"
        env: ""
        restart_policy:
          backoff_strategy:
            type: linear
            backoff_delay_seconds: 3
            max_retries: 8
            last_retry_interval_seconds: 60
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
    //             backoff_delay_seconds: Duration::from_secs(1),
    //             max_retries: 3,
    //             last_retry_interval_seconds: Duration::from_secs(30),
    //         }),
    //         on_host.restart_policy.backoff_strategy
    //     );
    // }

    #[test]
    fn test_basic_raw_agent_parsing() {
        let agent: RawAgent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

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

        // Restart restart policy values
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("fixed".to_string()),
                backoff_delay_seconds: TemplateableValue::from_template("1".to_string()),
                max_retries: TemplateableValue::from_template("3".to_string()),
                last_retry_interval_seconds: TemplateableValue::from_template("30".to_string()),
            },
            on_host.executables[0].restart_policy.backoff_strategy
        );
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("linear".to_string()),
                backoff_delay_seconds: TemplateableValue::from_template("3".to_string()),
                max_retries: TemplateableValue::from_template("8".to_string()),
                last_retry_interval_seconds: TemplateableValue::from_template("60".to_string()),
            },
            on_host.executables[1].restart_policy.backoff_strategy
        );
    }

    #[test]
    fn test_bad_parsing() {
        let raw_agent_err: Result<RawAgent, Error> = serde_yaml::from_str(AGENT_GIVEN_BAD_YAML);

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

        let given_agent: FinalAgent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        let expected_map: Map<String, EndSpec> = Map::from([(
            "description.name".to_string(),
            EndSpec {
                description: "Name of the agent".to_string(),
                type_: VariableType::String,
                required: false,
                default: Some(TrivialValue::String("nrdot".to_string())),
                final_value: None,
                file_path: None,
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.variables);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: VariableType::String,
            required: false,
            default: Some(TrivialValue::String("nrdot".to_string())),
            final_value: None,
            file_path: None,
        };

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
                "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: TemplateableValue::from_template("".to_string()),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template("${backoff.type}".to_string()),
                    backoff_delay_seconds: TemplateableValue::from_template(
                        "${backoff.delay}".to_string(),
                    ),
                    max_retries: TemplateableValue::from_template("${backoff.retries}".to_string()),
                    last_retry_interval_seconds: TemplateableValue::from_template(
                        "${backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let normalized_values = Map::from([
            (
                "bin".to_string(),
                EndSpec {
                    default: None,
                    description: "binary".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("/etc".to_string())),
                    file_path: None,
                },
            ),
            (
                "deployment.on_host.verbose".to_string(),
                EndSpec {
                    default: None,
                    description: "verbosity".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("true".to_string())),
                    file_path: None,
                },
            ),
            (
                "deployment.on_host.log_level".to_string(),
                EndSpec {
                    default: None,
                    description: "log_level".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("trace".to_string())),
                    file_path: None,
                },
            ),
            (
                "backoff.type".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_type".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("exponential".to_string())),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.delay".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_delay".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(10))),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.retries".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_retries".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(30))),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.interval".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_interval".to_string(),
                    type_: VariableType::Number,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(300))),
                    file_path: Some("some_path".to_string()),
                },
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue {
                value: Some("/etc/otelcol".to_string()),
                template: "${bin}/otelcol".to_string(),
            },
            args: TemplateableValue {
                value: Some(Args("--verbose true --logs trace".to_string())),
                template:
                    "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
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
                    backoff_delay_seconds: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(10)),
                        template: "${backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30),
                        template: "${backoff.retries}".to_string(),
                    },
                    last_retry_interval_seconds: TemplateableValue {
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
            restart_policy: RestartPolicyConfig{
                backoff_strategy: BackoffStrategyConfig{
                    backoff_type: TemplateableValue::from_template(
                        "${backoff.type}"
                            .to_string(),
                    ),
                    backoff_delay_seconds: TemplateableValue::from_template(
                        "${backoff.delay}"
                            .to_string(),
                    ),
                    max_retries: TemplateableValue::from_template(
                        "${backoff.retries}"
                            .to_string(),
                    ),
                    last_retry_interval_seconds: TemplateableValue::from_template(
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
                EndSpec {
                    default: None,
                    description: "binary".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("/etc".to_string())),
                    file_path: None,
                },
            ),
            (
                "deployment.on_host.verbose".to_string(),
                EndSpec {
                    default: None,
                    description: "verbosity".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("true".to_string())),
                    file_path: None,
                },
            ),
            (
                "backoff.type".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_type".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::String("linear".to_string())),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.delay".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_delay".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(10))),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.retries".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_retries".to_string(),
                    type_: VariableType::String,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(30))),
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.interval".to_string(),
                EndSpec {
                    default: None,
                    description: "backoff_interval".to_string(),
                    type_: VariableType::Number,
                    required: true,
                    final_value: Some(TrivialValue::Number(PosInt(300))),
                    file_path: Some("some_path".to_string()),
                },
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue{value: Some("/etc/otelcol".to_string()), template: "${bin}/otelcol".to_string()},
            args: TemplateableValue{value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string()},
            env: TemplateableValue{value: Some(Env("".to_string())), template: "".to_string()},
            restart_policy: RestartPolicyConfig{
                backoff_strategy: BackoffStrategyConfig{
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Linear),
                        template: "${backoff.type}".to_string(),
                    },
                    backoff_delay_seconds: TemplateableValue {
                        value: Some(BackoffDuration::from_secs(10)),
                        template: "${backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30),
                        template: "${backoff.retries}".to_string(),
                    },
                    last_retry_interval_seconds: TemplateableValue {
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
    fn test_template_with_runtime_field() {
        // Having Agent Type
        let input_agent_type =
            serde_yaml::from_str::<FinalAgent>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();

        // And Agent Values
        let input_user_config =
            serde_yaml::from_str::<SupervisorConfig>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML)
                .unwrap();

        // When populating values
        let actual = input_agent_type
            .template_with(input_user_config)
            .expect("Failed to template_with the AgentType's runtime_config field");

        // Then we expected final values
        // MapStringString
        let expected_config_3 = TrivialValue::Map(HashMap::from([
            (
                "log_level".to_string(),
                TrivialValue::String("trace".to_string()),
            ),
            (
                "forward".to_string(),
                TrivialValue::String("true".to_string()),
            ),
        ]));
        // File with default
        let expected_config_2 = TrivialValue::File(FilePathWithContent::new(
            "config2.yml".to_string(),
            "license_key: abc123\nstaging: true\n".to_string(),
        ));
        // File with values
        let expected_config = TrivialValue::File(FilePathWithContent::new(
            "config.yml".to_string(),
            "license_key: abc124\nstaging: false\n".to_string(),
        ));
        // MapStringFile
        let expected_integrations = TrivialValue::Map(HashMap::from([
            (
                "kafka.conf".to_string(),
                TrivialValue::File(FilePathWithContent::new(
                    "integrations.d".to_string(),
                    "strategy: bootstrap\n".to_string(),
                )),
            ),
            (
                "redis.yml".to_string(),
                TrivialValue::File(FilePathWithContent::new(
                    "integrations.d".to_string(),
                    "user: redis\n".to_string(),
                )),
            ),
        ]));

        assert_eq!(
            expected_config_3,
            actual
                .get_variables()
                .get("config3")
                .unwrap()
                .final_value
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
                .final_value
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
                .final_value
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
                .final_value
                .as_ref()
                .unwrap()
                .clone()
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
      type: number
      required: false
      default: 1
    retries:
      description: "Backoff retries"
      type: number
      required: false
      default: 3
    interval:
      description: "Backoff interval"
      type: number
      required: false
      default: 30
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
            backoff_delay_seconds: ${backoff.delay}
            max_retries: ${backoff.retries}
            last_retry_interval_seconds: ${backoff.interval}
"#;

    const BACKOFF_CONFIG_YAML: &str = r#"
backoff:
  delay: 10
  retries: 30
  interval: 300
  type: linear
"#;

    #[test]
    fn test_backoff_config() {
        let input_agent_type =
            serde_yaml::from_str::<FinalAgent>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();
        // println!("Input: {:#?}", input_agent_type);

        let input_user_config =
            serde_yaml::from_str::<SupervisorConfig>(BACKOFF_CONFIG_YAML).unwrap();
        // println!("Input: {:#?}", input_user_config);

        let expected_backoff = BackoffStrategyConfig {
            backoff_type: TemplateableValue {
                value: Some(BackoffStrategyType::Linear),
                template: "${backoff.type}".to_string(),
            },
            backoff_delay_seconds: TemplateableValue {
                value: Some(BackoffDuration::from_secs(10)),
                template: "${backoff.delay}".to_string(),
            },
            max_retries: TemplateableValue {
                value: Some(30),
                template: "${backoff.retries}".to_string(),
            },
            last_retry_interval_seconds: TemplateableValue {
                value: Some(BackoffDuration::from_secs(300)),
                template: "${backoff.interval}".to_string(),
            },
        };

        let actual = input_agent_type
            .template_with(input_user_config)
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
            serde_yaml::from_str::<FinalAgent>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();

        let wrong_retries =
            serde_yaml::from_str::<SupervisorConfig>(WRONG_RETRIES_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_delay =
            serde_yaml::from_str::<SupervisorConfig>(WRONG_DELAY_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_interval =
            serde_yaml::from_str::<SupervisorConfig>(WRONG_INTERVAL_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_type =
            serde_yaml::from_str::<SupervisorConfig>(WRONG_TYPE_BACKOFF_CONFIG_YAML).unwrap();

        let actual = input_agent_type.clone().template_with(wrong_retries);
        assert!(actual.is_err());

        let actual = input_agent_type.clone().template_with(wrong_delay);
        assert!(actual.is_err());

        let actual = input_agent_type.clone().template_with(wrong_interval);
        assert!(actual.is_err());

        let actual = input_agent_type.template_with(wrong_type);
        assert!(actual.is_err());
    }

    // Obsolete test

    /*
    const EXAMPLE_AGENT_YAML_REPLACE_WITH_DEFAULT: &str = r#"
    name: nrdot
    namespace: newrelic
    version: 0.1.0
    variables:
      config:
        description: "Path to the agent"
        type: file
        required: false
        default: "test"
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
            required: false
            default: "--verbose true"
      integrations:
        description: "Newrelic integrations configuration yamls"
        type: map[string]file
        required: false
        default:
          kafka: |
            bootstrap: zookeeper
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
            serde_yaml::from_str::<SupervisorConfig>("").unwrap();
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
            (
                "config".to_string(),
                TrivialValue::File(FilePathWithContent::new("test".to_string())),
            ),
            (
                "integrations".to_string(),
                TrivialValue::Map(Map::from([(
                    "kafka".to_string(),
                    TrivialValue::File(FilePathWithContent::new(
                        "bootstrap: zookeeper\n".to_string(),
                    )),
                )])),
            ),
        ]);
        let actual = agent_type
            .populate(input_structure)
            .expect("Failed to populate the AgentType's runtime_config field");
        expected.iter().for_each(|(key, expected_value)|{
            let actual_value = actual.clone().get_variables(key.to_string()).unwrap().final_value;
            if let Some(TrivialValue::File(actual_file)) = actual_value {
                let TrivialValue::File(expected_file) = expected_value else { unreachable!() };
                assert_eq!(expected_file.content, actual_file.content);
            } else if let Some(TrivialValue::Map(actual_map)) = actual_value {
                actual_map.iter().for_each(|(a,actual_map)|{
                    let TrivialValue::File(actual_file) = actual_map else { unreachable!() };
                    let TrivialValue::Map(expected_map) = expected_value else { unreachable!() };
                    let TrivialValue::File(expected_file) = expected_map.get(a).unwrap() else { unreachable!() };
                    assert_eq!(expected_file.content, actual_file.content);
                });
            } else {
                assert_eq!(*expected_value, actual_value.unwrap())
            }
        });
    }
    */
}
