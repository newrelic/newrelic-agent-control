//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Super Agent will be running.
//!
//! The reasoning behind this is that the Super Agent will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use std::fmt::{Display, Formatter};
use std::{collections::HashMap, str::FromStr};

use crate::config::super_agent_configs::AgentTypeFQN;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::restart_policy::BackoffDuration;
use super::trivial_value::{FilePathWithContent, Number};
use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    runtime_config::{Args, Env, RuntimeConfig},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
    trivial_value::TrivialValue,
};
use crate::config::agent_values::AgentValues;
use duration_str;

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to start given agent binaries.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`RawAgent`]) having the spec field normalized.
///
/// See also [`RawAgent`] and the [`FinalAgent::try_from`] implementation.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct FinalAgent {
    pub metadata: AgentMetadata,
    pub variables: NormalizedVariables,
    pub runtime_config: RuntimeConfig,
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
            variables: normalize_agent_spec(raw_agent.variables).map_err(D::Error::custom)?,
            metadata: raw_agent.metadata,
            runtime_config: raw_agent.runtime_config, // FIXME: make it actual implementation
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

    pub fn get_variables(&self) -> &NormalizedVariables {
        &self.variables
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// template_with the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    pub fn template_with(self, config: AgentValues) -> Result<FinalAgent, AgentTypeError> {
        // let normalized_config = NormalizedSupervisorConfig::from(config);
        // let validated_conf = validate_with_agent_type(normalized_config, &self)?;
        let config = config.normalize_with_agent_type(&self)?;

        // let runtime_conf = self.runtime_config.template_with(validated_conf.clone())?;
        let mut spec = self.variables;

        // modifies variables final value with the one defined in the SupervisorConfig
        spec.iter_mut()
            .try_for_each(|(k, v)| -> Result<(), AgentTypeError> {
                let defined_value = config.get_from_normalized(k);
                v.kind.set_final_value(defined_value)?;
                Ok(())
            })?;

        let runtime_conf = self.runtime_config.template_with(&spec)?;

        let populated_agent = FinalAgent {
            runtime_config: runtime_conf,
            variables: spec,
            ..self
        };

        Ok(populated_agent)
    }
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`AgentValues`].
type AgentVariables = HashMap<String, Spec>;

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

impl Display for VariableType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableType::String => write!(f, "string"),
            VariableType::Bool => write!(f, "bool"),
            VariableType::Number => write!(f, "number"),
            VariableType::File => write!(f, "file"),
            VariableType::MapStringString => write!(f, "map[string]string"),
            VariableType::MapStringFile => write!(f, "map[string]file"),
            // VariableType::MapStringNumber => write!(f, "map[string]number"),
            // VariableType::MapStringBool => write!(f, "map[string]bool"),
        }
    }
}

pub trait AgentTypeEndSpec {
    fn variable_type(&self) -> VariableType;
    fn file_path(&self) -> Option<String>;
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct EndSpec {
    pub(crate) description: String,
    #[serde(flatten)]
    pub kind: Kind,
    pub(crate) file_path: Option<String>,
}

impl AgentTypeEndSpec for EndSpec {
    fn variable_type(&self) -> VariableType {
        match self.kind {
            Kind::String(_) => VariableType::String,
            Kind::Bool(_) => VariableType::Bool,
            Kind::Number(_) => VariableType::Number,
            Kind::File(_) => VariableType::File,
            Kind::MapStringFile(_) => VariableType::MapStringFile,
            Kind::MapStringString(_) => VariableType::MapStringString,
        }
    }

    fn file_path(&self) -> Option<String> {
        self.file_path.clone()
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(tag = "type")]
pub enum Kind {
    #[serde(rename = "string")]
    String(KindValue<String>),
    #[serde(rename = "bool")]
    Bool(KindValue<bool>),
    #[serde(rename = "number")]
    Number(KindValue<Number>),
    #[serde(rename = "file")]
    File(KindValue<FilePathWithContent>),
    #[serde(rename = "map[string]string")]
    MapStringString(KindValue<HashMap<String, String>>),
    #[serde(rename = "map[string]file")]
    MapStringFile(KindValue<HashMap<String, FilePathWithContent>>),
}

impl Kind {
    fn kind_str(&self) -> &str {
        match self {
            Kind::String(_) => "string",
            Kind::Bool(_) => "bool",
            Kind::Number(_) => "number",
            Kind::File(_) => "file",
            Kind::MapStringFile(_) => "map[string]file",
            Kind::MapStringString(_) => "map[string]string",
        }
    }

    fn not_required_without_default(&self) -> bool {
        match self {
            Kind::String(v) => v.not_required_without_default(),
            Kind::Bool(v) => v.not_required_without_default(),
            Kind::Number(v) => v.not_required_without_default(),
            Kind::File(v) => v.not_required_without_default(),
            Kind::MapStringFile(v) => v.not_required_without_default(),
            Kind::MapStringString(v) => v.not_required_without_default(),
        }
    }

    pub(crate) fn required(&self) -> bool {
        match self {
            Kind::String(v) => v.required,
            Kind::Bool(v) => v.required,
            Kind::Number(v) => v.required,
            Kind::File(v) => v.required,
            Kind::MapStringFile(v) => v.required,
            Kind::MapStringString(v) => v.required,
        }
    }

    fn set_final_value(&mut self, value: Option<TrivialValue>) -> Result<(), AgentTypeError> {
        if let Some(v) = value {
            match (self, v) {
                (Kind::String(v), TrivialValue::String(s)) => v.final_value = Some(s),
                (Kind::Bool(v), TrivialValue::Bool(b)) => v.final_value = Some(b),
                (Kind::Number(v), TrivialValue::Number(n)) => v.final_value = Some(n),
                // FIXME
                (k, v) => {
                    return Err(AgentTypeError::TypeMismatch {
                        expected_type: k.kind_str().to_string(),
                        actual_value: v,
                    })
                }
            }
        } else {
            self.set_default_as_final_value();
        }
        Ok(())
    }

    fn set_default_as_final_value(&mut self) {
        match self {
            Kind::String(v) => v.set_default_as_final(),
            Kind::Bool(v) => v.set_default_as_final(),
            Kind::Number(v) => v.set_default_as_final(),
            Kind::File(v) => v.set_default_as_final(),
            Kind::MapStringFile(v) => v.set_default_as_final(),
            Kind::MapStringString(v) => v.set_default_as_final(),
        }
    }

    pub(crate) fn get_final_value(&self) -> Option<TrivialValue> {
        match self {
            Kind::String(v) => v.final_value.clone().map(TrivialValue::String),
            Kind::Bool(v) => v.final_value.map(TrivialValue::Bool),
            Kind::Number(v) => v.final_value.clone().map(TrivialValue::Number),
            Kind::File(v) => v.final_value.clone().map(TrivialValue::File),
            Kind::MapStringFile(v) => v.final_value.clone().map(|vv| {
                let trivial_value_map = vv
                    .into_iter()
                    .map(|(k, v)| (k, TrivialValue::File(v)))
                    .collect();
                TrivialValue::Map(trivial_value_map)
            }),
            Kind::MapStringString(v) => v.final_value.clone().map(|vv| {
                let trivial_value_map = vv
                    .into_iter()
                    .map(|(k, v)| (k, TrivialValue::String(v)))
                    .collect();
                TrivialValue::Map(trivial_value_map)
            }),
        }
    }

    pub(crate) fn get_default(&self) -> Option<TrivialValue> {
        match self {
            Kind::String(v) => v.default.clone().map(TrivialValue::String),
            Kind::Bool(v) => v.default.map(TrivialValue::Bool),
            Kind::Number(v) => v.default.clone().map(TrivialValue::Number),
            Kind::File(v) => v.default.clone().map(TrivialValue::File),
            Kind::MapStringFile(v) => v.default.clone().map(|vv| {
                let trivial_value_map = vv
                    .into_iter()
                    .map(|(k, v)| (k, TrivialValue::File(v)))
                    .collect();
                TrivialValue::Map(trivial_value_map)
            }),
            Kind::MapStringString(v) => v.default.clone().map(|vv| {
                let trivial_value_map = vv
                    .into_iter()
                    .map(|(k, v)| (k, TrivialValue::String(v)))
                    .collect();
                TrivialValue::Map(trivial_value_map)
            }),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct KindValue<T> {
    pub(crate) default: Option<T>,
    pub(crate) final_value: Option<T>,
    pub required: bool,
    pub(crate) variants: Option<Vec<T>>,
}

impl<T> KindValue<T> {
    pub(crate) fn not_required_without_default(&self) -> bool {
        !self.required && self.default.is_none()
    }
    pub(crate) fn set_default_as_final(&mut self) {
        self.final_value = self.default.take();
    }
}

impl<'de, T> Deserialize<'de> for KindValue<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // temporal type for intermediate serialization
        #[derive(Debug, Deserialize)]
        struct IntermediateValueKind<T> {
            default: Option<T>,
            variants: Option<Vec<T>>,
            required: bool,
        }

        let intermediate_spec = IntermediateValueKind::deserialize(deserializer)?;
        if intermediate_spec.default.is_none() && !intermediate_spec.required {
            let err = D::Error::custom(AgentTypeError::MissingDefault);
            return Err(err);
        }

        Ok(KindValue {
            default: intermediate_spec.default,
            required: intermediate_spec.required,
            final_value: None,
            variants: intermediate_spec.variants,
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
            if end_spec.kind.not_required_without_default() {
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
        },
        agent_values::AgentValues,
    };

    use super::*;
    use crate::config::agent_type::restart_policy::RestartPolicyConfig;
    use crate::config::agent_type::trivial_value::FilePathWithContent;
    use serde_yaml::Error;
    use std::collections::HashMap as Map;

    impl FinalAgent {
        pub fn new(
            metadata: AgentMetadata,
            variables: NormalizedVariables,
            runtime_config: RuntimeConfig,
        ) -> FinalAgent {
            FinalAgent {
                metadata,
                variables,
                runtime_config,
            }
        }

        /// Retrieve the `variables` field of the agent type at the specified key, if any.
        pub fn get_variable(self, path: String) -> Option<EndSpec> {
            self.variables.get(&path).cloned()
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

        let endspec: EndSpec = EndSpec {
            description: "Name of the agent".to_string(),
            kind: Kind::String(KindValue {
                final_value: None,
                required: false,
                default: Some("nrdot".to_string()),
                variants: None,
            }),
            file_path: None,
        };

        let given_agent: FinalAgent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        let expected_map: Map<String, EndSpec> =
            Map::from([("description.name".to_string(), endspec.clone())]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.variables);

        let expected_spec = endspec;

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
                    backoff_delay: TemplateableValue::from_template("${backoff.delay}".to_string()),
                    max_retries: TemplateableValue::from_template("${backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::from_template(
                        "${backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let bin_endspec: EndSpec = EndSpec {
            description: "binary".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("/etc".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: None,
        };
        let verbosity_endspec: EndSpec = EndSpec {
            description: "verbosity".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("true".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: None,
        };
        let loglevel_endspec: EndSpec = EndSpec {
            description: "log_level".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("trace".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: None,
        };
        let backofftype_endspec: EndSpec = EndSpec {
            description: "backoff_type".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("exponential".to_string()),
                required: true,
                default: None,
                variants: None, // FIXME???
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffdelay_endspec: EndSpec = EndSpec {
            description: "backoff_delay".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("10s".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffretries_endspec: EndSpec = EndSpec {
            description: "backoff_retries".to_string(),
            kind: Kind::Number(KindValue {
                final_value: Some(Number::PosInt(30)),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffinterval_endspec: EndSpec = EndSpec {
            description: "backoff_interval".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("300s".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };

        let normalized_values = Map::from([
            ("bin".to_string(), bin_endspec),
            ("deployment.on_host.verbose".to_string(), verbosity_endspec),
            ("deployment.on_host.log_level".to_string(), loglevel_endspec),
            ("backoff.type".to_string(), backofftype_endspec),
            ("backoff.delay".to_string(), backoffdelay_endspec),
            ("backoff.retries".to_string(), backoffretries_endspec),
            ("backoff.interval".to_string(), backoffinterval_endspec),
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

        let bin_endspec: EndSpec = EndSpec {
            description: "binary".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("/etc".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: None,
        };
        let verbosity_endspec: EndSpec = EndSpec {
            description: "verbosity".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("true".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: None,
        };
        let backofftype_endspec: EndSpec = EndSpec {
            description: "backoff_type".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("exponential".to_string()),
                required: true,
                default: None,
                variants: None, // FIXME???
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffdelay_endspec: EndSpec = EndSpec {
            description: "backoff_delay".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("10s".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffretries_endspec: EndSpec = EndSpec {
            description: "backoff_retries".to_string(),
            kind: Kind::Number(KindValue {
                final_value: Some(Number::PosInt(30)),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };
        let backoffinterval_endspec: EndSpec = EndSpec {
            description: "backoff_interval".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("300s".to_string()),
                required: true,
                default: None,
                variants: None,
            }),
            file_path: Some("some_path".to_string()),
        };

        let normalized_values = Map::from([
            ("bin".to_string(), bin_endspec),
            ("deployment.on_host.verbose".to_string(), verbosity_endspec),
            ("backoff.type".to_string(), backofftype_endspec),
            ("backoff.delay".to_string(), backoffdelay_endspec),
            ("backoff.retries".to_string(), backoffretries_endspec),
            ("backoff.interval".to_string(), backoffinterval_endspec),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue { value: Some("/etc/otelcol".to_string()), template: "${bin}/otelcol".to_string() },
            args: TemplateableValue { value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string() },
            env: TemplateableValue { value: Some(Env("".to_string())), template: "".to_string() },
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
            serde_yaml::from_str::<AgentValues>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML).unwrap();

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
            serde_yaml::from_str::<AgentValues>(WRONG_RETRIES_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_delay =
            serde_yaml::from_str::<AgentValues>(WRONG_DELAY_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_interval =
            serde_yaml::from_str::<AgentValues>(WRONG_INTERVAL_BACKOFF_CONFIG_YAML).unwrap();
        let wrong_type =
            serde_yaml::from_str::<AgentValues>(WRONG_TYPE_BACKOFF_CONFIG_YAML).unwrap();

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
}
