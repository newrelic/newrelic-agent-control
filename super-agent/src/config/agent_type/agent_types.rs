//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Super Agent will be running.
//!
//! The reasoning behind this is that the Super Agent will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use crate::config::agent_type::variable_spec::spec::Spec;
use crate::config::super_agent_configs::AgentTypeFQN;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::fmt::Display;
use std::{collections::HashMap, str::FromStr};

use super::restart_policy::BackoffDuration;
use super::trivial_value::{FilePathWithContent, TrivialValue};
use super::variable_spec::kind_value::KindValue;
use super::variable_spec::spec::EndSpec;
use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    runtime_config::{Args, Env, RuntimeConfig},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
};
use crate::config::agent_values::AgentValues;
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
pub struct FinalAgent {
    pub metadata: AgentMetadata,
    // pub variables: NormalizedVariables,
    pub variables: AgentVariables,
    pub runtime_config: RuntimeConfig,
    capabilities: Capabilities,
}

impl FinalAgent {
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

impl<'de> Deserialize<'de> for FinalAgent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // temporal type for raw deserialization
        #[derive(Debug, Deserialize)]
        struct RawAgent {
            #[serde(flatten)]
            metadata: AgentMetadata,
            variables: AgentVariables,
            #[serde(default, flatten)]
            runtime_config: RuntimeConfig,
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

impl FinalAgent {
    pub fn agent_type(&self) -> AgentTypeFQN {
        self.metadata.to_string().as_str().into()
    }

    pub fn get_variables(&self) -> NormalizedVariables {
        self.variables.clone().flatten()
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// template_with the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    pub fn template_with(
        mut self,
        config: AgentValues,
        agent_configs_path: Option<&str>,
    ) -> Result<FinalAgent, AgentTypeError> {
        // let normalized_config = NormalizedSupervisorConfig::from(config);
        // let validated_conf = validate_with_agent_type(normalized_config, &self)?;
        let config = config.normalize_with_agent_type(&mut self)?;

        // let runtime_conf = self.runtime_config.template_with(validated_conf.clone())?;
        // let mut spec = config.variables;

        // // modifies variables final value with the one defined in the SupervisorConfig
        // spec.0
        //     .iter_mut()
        //     .try_for_each(|(k, v)| -> Result<(), AgentTypeError> {
        //         // let defined_value = config.get_from_normalized(k);
        //         // v.kind.set_final_value(defined_value)?;
        //         match config.get_from_normalized(k) {
        //             Some(value) => v.kind.set_final_value(value),
        //             None => Ok(v.kind.set_default_as_final()),
        //         }
        //     })?;

        let runtime_conf = self
            .runtime_config
            .template_with(&self.variables.clone().flatten())?;

        let populated_agent = FinalAgent {
            runtime_config: runtime_conf,
            // variables: spec,
            ..self
        };

        Ok(populated_agent)
    }
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`AgentValues`].
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AgentVariables(pub(crate) HashMap<String, Spec>);

impl AgentVariables {
    pub fn flatten(self) -> HashMap<String, EndSpec> {
        self.0
            .into_iter()
            .flat_map(|(k, v)| inner_flatten(k, v))
            .collect()
    }
}

fn inner_flatten(key: String, spec: Spec) -> HashMap<String, EndSpec> {
    let mut result = HashMap::new();
    match spec {
        Spec::SpecEnd(s) => _ = result.insert(key, s),
        Spec::SpecMapping(m) => m.into_iter().for_each(|(k, v)| {
            result.extend(inner_flatten(key.clone() + TEMPLATE_KEY_SEPARATOR + &k, v))
        }),
    }
    result
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
    #[serde(rename = "yaml")]
    Yaml,
}

impl Display for VariableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableType::String => write!(f, "string"),
            VariableType::Bool => write!(f, "bool"),
            VariableType::Number => write!(f, "number"),
            VariableType::File => write!(f, "file"),
            VariableType::MapStringString => write!(f, "map[string]string"),
            VariableType::MapStringFile => write!(f, "map[string]file"),
            // VariableType::MapStringNumber => write!(f, "map[string]number"),
            // VariableType::MapStringBool => write!(f, "map[string]bool"),
            VariableType::Yaml => write!(f, "yaml"),
        }
    }
}

pub trait AgentTypeEndSpec {
    fn variable_type(&self) -> VariableType;
    fn file_path(&self) -> Option<String>;
}

impl EndSpec {
    /// get_template_value returns the replacement value that will be used to substitute
    /// the placeholder from an agent_type when templating a config
    pub fn get_template_value(&self) -> Option<TrivialValue> {
        match self.kind.variable_type() {
            // For MapStringFile and file the file_path includes the full path with agent_configs_path
            VariableType::MapStringFile => {
                let inner_value: KindValue<HashMap<String, FilePathWithContent>> = (&self.kind)
                    .try_into()
                    .expect("A type of map[string]file must have a file path at this point");
                if let Some(file_path) = inner_value.file_path {
                    return Some(TrivialValue::String(
                        file_path.to_string_lossy().to_string(),
                    ));
                }
                inner_value.default.map(TrivialValue::MapStringFile).into()
            }
            VariableType::File => {
                let inner_value: KindValue<FilePathWithContent> = (&self.kind)
                    .try_into()
                    .expect("A type of file must have a file path at this point");
                if let Some(file_path) = inner_value.file_path {
                    return Some(TrivialValue::String(
                        file_path.to_string_lossy().to_string(),
                    ));
                }
                inner_value.default.map(TrivialValue::File).into()
            }
            _ => self.kind.get_final_value(),
        }
    }
}

// impl<'de> Deserialize<'de> for EndSpec {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         // temporal type for intermediate serialization
//         #[derive(Debug, Deserialize)]
//         struct IntermediateEndSpec {
//             description: String,
//             #[serde(rename = "type")]
//             type_: VariableType,
//             required: bool,
//             default: Option<TrivialValue>,
//             file_path: Option<String>,
//         }

//         impl AgentTypeEndSpec for IntermediateEndSpec {
//             fn variable_type(&self) -> VariableType {
//                 self.type_
//             }

//             fn file_path(&self) -> Option<String> {
//                 self.file_path.as_ref().cloned()
//             }
//         }

//         let intermediate_spec = IntermediateEndSpec::deserialize(deserializer)?;
//         if intermediate_spec.default.is_none() && !intermediate_spec.required {
//             return Err(D::Error::custom(AgentTypeError::MissingDefault));
//         }
//         let def_val = intermediate_spec
//             .default
//             .clone()
//             .map(|d| d.check_type(&intermediate_spec))
//             .transpose()
//             .map_err(D::Error::custom)?;

//         Ok(EndSpec {
//             default: def_val,
//             final_value: None,
//             file_path: intermediate_spec.file_path,
//             description: intermediate_spec.description,
//             type_: intermediate_spec.type_,
//             required: intermediate_spec.required,
//         })
//     }
// }

// impl AgentTypeEndSpec for EndSpec {
//     fn variable_type(&self) -> VariableType {
//         self.type_
//     }

//     fn file_path(&self) -> Option<String> {
//         self.file_path.as_ref().cloned()
//     }
// }

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct K8s {
    crd: String,
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
    spec.0.into_iter().try_fold(HashMap::new(), |r, (k, v)| {
        let n_spec = inner_normalize(k, v);
        n_spec.iter().try_for_each(|(k, end_spec)| {
            if end_spec.kind.is_not_required_without_default() {
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
pub mod tests {
    use crate::config::{
        agent_type::{
            restart_policy::{BackoffStrategyConfig, BackoffStrategyType},
            runtime_config::{Args, Env, Executable},
            trivial_value::TrivialValue,
            variable_spec::kind_value::KindValue,
        },
        agent_values::AgentValues,
    };

    use super::*;
    use crate::config::agent_type::restart_policy::RestartPolicyConfig;
    use crate::config::agent_type::trivial_value::FilePathWithContent;
    use crate::config::agent_type::trivial_value::Number::PosInt;
    use serde_yaml::Error;
    use std::collections::HashMap as Map;

    impl FinalAgent {
        // pub fn new(
        //     metadata: AgentMetadata,
        //     variables: NormalizedVariables,
        //     runtime_config: RuntimeConfig,
        // ) -> FinalAgent {
        //     FinalAgent {
        //         metadata,
        //         variables,
        //         runtime_config,
        //         capabilities: default_capabilities(),
        //     }
        // }

        pub fn set_capabilities(&mut self, capabilities: Capabilities) {
            self.capabilities = capabilities
        }

        /// Retrieve the `variables` field of the agent type at the specified key, if any.
        pub fn get_variable(self, path: String) -> Option<EndSpec> {
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
        let agent: FinalAgent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

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
        let raw_agent_err: Result<FinalAgent, Error> = serde_yaml::from_str(AGENT_GIVEN_BAD_YAML);

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
                kind: KindValue {
                    required: false,
                    default: Some("nrdot".to_string()),
                    final_value: None,
                    file_path: None,
                }
                .into(),
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.variables.clone().flatten());

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            kind: KindValue {
                required: false,
                default: Some("nrdot".to_string()),
                final_value: None,
                file_path: None,
            }
            .into(),
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
                EndSpec {
                    description: "binary".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("/etc".to_string()),
                        file_path: None,
                    }
                    .into(),
                },
            ),
            (
                "config".to_string(),
                EndSpec {
                    description: "config".to_string(),
                    kind: KindValue {
                        required: true,
                        default: None,
                        final_value: Some(FilePathWithContent::new(
                            "config2.yml".to_string(),
                            "license_key: abc123\nstaging: true\n".to_string(),
                        )),
                        file_path: Some("config_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "integrations".to_string(),
                EndSpec {
                    description: "integrations".to_string(),
                    kind: KindValue {
                        final_value: Some(HashMap::from([
                            (
                                "kafka.yml".to_string(),
                                FilePathWithContent::new(
                                    "config2.yml".to_string(),
                                    "license_key: abc123\nstaging: true\n".to_string(),
                                ),
                            ),
                            (
                                "redis.yml".to_string(),
                                FilePathWithContent::new(
                                    "config2.yml".to_string(),
                                    "license_key: abc123\nstaging: true\n".to_string(),
                                ),
                            ),
                        ])),
                        default: None,
                        required: true,
                        file_path: Some("integration_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "deployment.on_host.verbose".to_string(),
                EndSpec {
                    description: "verbosity".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("true".to_string()),
                        file_path: None,
                    }
                    .into(),
                },
            ),
            (
                "deployment.on_host.log_level".to_string(),
                EndSpec {
                    description: "log_level".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("trace".to_string()),
                        file_path: None,
                    }
                    .into(),
                },
            ),
            (
                "backoff.type".to_string(),
                EndSpec {
                    description: "backoff_type".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("exponential".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.delay".to_string(),
                EndSpec {
                    description: "backoff_delay".to_string(),
                    kind: KindValue {
                        required: true,
                        default: None,
                        final_value: Some("10s".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.retries".to_string(),
                EndSpec {
                    description: "backoff_retries".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some(PosInt(30)),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.interval".to_string(),
                EndSpec {
                    description: "backoff_interval".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("300s".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
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
                EndSpec {
                    description: "binary".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("/etc".to_string()),
                        file_path: None,
                    }
                    .into(),
                },
            ),
            (
                "deployment.on_host.verbose".to_string(),
                EndSpec {
                    description: "verbosity".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("true".to_string()),
                        file_path: None,
                    }
                    .into(),
                },
            ),
            (
                "backoff.type".to_string(),
                EndSpec {
                    description: "backoff_type".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("linear".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.delay".to_string(),
                EndSpec {
                    description: "backoff_delay".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("10s".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.retries".to_string(),
                EndSpec {
                    description: "backoff_retries".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some(PosInt(30)),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
            ),
            (
                "backoff.interval".to_string(),
                EndSpec {
                    description: "backoff_interval".to_string(),
                    kind: KindValue {
                        default: None,
                        required: true,
                        final_value: Some("300s".to_string()),
                        file_path: Some("some_path".into()),
                    }
                    .into(),
                },
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
            serde_yaml::from_str::<FinalAgent>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();

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
            "config2.yml".to_string(),
            "license_key: abc123\nstaging: true\n".to_string(),
        )
        .into();
        // File with values
        let expected_config: TrivialValue = FilePathWithContent::new(
            "config.yml".to_string(),
            "license_key: abc124\nstaging: false\n".to_string(),
        )
        .into();
        // MapStringFile
        let expected_integrations: TrivialValue = HashMap::from([
            (
                "kafka.conf".to_string(),
                FilePathWithContent::new(
                    "integrations.d".to_string(),
                    "strategy: bootstrap\n".to_string(),
                ),
            ),
            (
                "redis.yml".to_string(),
                FilePathWithContent::new("integrations.d".to_string(), "user: redis\n".to_string()),
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
                .kind
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
                .kind
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
                .kind
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
                .kind
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
            serde_yaml::from_str::<FinalAgent>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();
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
            serde_yaml::from_str::<FinalAgent>(AGENT_BACKOFF_TEMPLATE_YAML).unwrap();

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
            serde_yaml::from_str::<FinalAgent>(AGENT_STRING_DURATIONS_TEMPLATE_YAML).unwrap();

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
  values: |
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
        let input_agent_type: FinalAgent =
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
}
