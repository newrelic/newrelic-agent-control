//! This module contains the definitions of the Supervisor's Agent Type, which is the type of agent that the Supervisor will be running.
//!
//! The reasoning behind this is that the Supervisor will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use std::{collections::HashMap, str::FromStr, time::Duration};

use duration_string::DurationString;
use serde::{Deserialize, Deserializer};
use serde_with::DeserializeFromStr;

use crate::config::supervisor_config::{
    validate_with_agent_type, NormalizedSupervisorConfig, SupervisorConfig,
};

use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    runtime_config::{Args, Env, RuntimeConfig},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
    trivial_value::TrivialValue,
};

#[derive(Debug, Deserialize)]
struct RawAgent {
    #[serde(flatten)]
    metadata: AgentMetadata,
    variables: AgentVariables,
    #[serde(default, flatten)]
    runtime_config: RuntimeConfig,
}

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to start given agent binaries.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`RawAgent`]) having the spec field normalized.
///
/// See also [`RawAgent`] and its [`Agent::try_from`] implementation.
#[derive(Debug, PartialEq, Clone, Default, Deserialize)]
#[serde(try_from = "RawAgent")]
pub struct FinalAgent {
    #[serde(flatten)]
    pub metadata: AgentMetadata,
    pub variables: NormalizedVariables,
    pub runtime_config: RuntimeConfig,
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
            template: result.to_string(),
        })
    }
}

impl<T> TemplateableValue<T> {
    // FIXME: make it take a reference
    pub fn get(self) -> Result<T, AgentTypeError> {
        self.value.ok_or(AgentTypeError::ValueNotPopulated)
    }
    #[cfg(test)]
    pub fn template(self) -> String {
        self.template
    }
    #[cfg(test)]
    pub fn from_template(s: String) -> Self {
        Self {
            value: None,
            template: s,
        }
    }
    pub fn new(value: T) -> Self {
        Self {
            value: Some(value),
            template: "".to_string(),
        }
    }
}

impl<S> Templateable for TemplateableValue<S>
where
    S: FromStr,
{
    fn template_with(self, kv: HashMap<String, TrivialValue>) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(kv)?;
        let value = templated_string
            .parse()
            .map_err(|_| AgentTypeError::ValueNotParseableFromString)?;
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<Env> {
    fn template_with(self, kv: HashMap<String, TrivialValue>) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(kv)?;
        Ok(Self {
            template: self.template,
            value: Some(Env(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<Args> {
    fn template_with(self, kv: HashMap<String, TrivialValue>) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(kv)?;
        Ok(Self {
            template: self.template,
            value: Some(Args(templated_string)),
        })
    }
}

impl FinalAgent {
    /// Retrieve the `variables` field of the agent type at the specified key, if any.
    pub fn get_variables(self, path: String) -> Option<EndSpec> {
        self.variables.get(&path).cloned()
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// template_with the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    ///
    /// The expected overall dataflow ending in `template_with`, with the functions involved, is the following:
    ///
    /// ```mermaid
    /// flowchart
    ///     subgraph main
    ///     A[User] --> |provides| B["Agent Type (YAML)"]
    ///     B --> |"parses into (serde)"| C[RawAgent]
    ///     C --> |"normalize_agent_spec()"| D[Agent]
    ///     D --> G{"Agent::template_with()"}
    ///     A --> |provides| E["Agent Config (YAML)"]
    ///     E --> |"parses into (serde)"| F[SupervisorConfig]
    ///     F --> G{"Agent::template_with()"}
    ///     end
    ///     subgraph "Agent::template_with()"
    ///     G -.-> H[SupervisorConfig]
    ///     G -.-> I[Agent]
    ///     H -->|"::from()"| J[NormalizedSupervisorConfig]
    ///     J --> K{"validate_with_agent_type()"}
    ///     I --> K
    ///     K --> L{{valid and type-checked config with all final values}}
    ///     end
    ///     subgraph templating
    ///     L -->|"::template_with(valid_config)"| M[updated Meta]
    ///     L --> N[updated Spec]
    ///     end
    ///     subgraph "Final Agent Supervisor"
    ///     M --> O{Agent}
    ///     N --> O
    ///     O --> |creates| P[Supervisor]
    ///     P --> Q(((RUN)))
    ///     end
    /// ```
    pub fn template_with(self, config: SupervisorConfig) -> Result<FinalAgent, AgentTypeError> {
        let normalized_config = NormalizedSupervisorConfig::from(config);
        let validated_conf = validate_with_agent_type(normalized_config, &self)?;

        let runtime_conf = self.runtime_config.template_with(validated_conf.clone())?;
        let mut spec = self.variables;

        validated_conf.into_iter().for_each(|(k, v)| {
            spec.entry(k).and_modify(|s| {
                s.final_value = Some(v);
            });
        });

        Ok(FinalAgent {
            metadata: self.metadata,
            variables: spec,
            runtime_config: runtime_conf,
        })
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
            runtime_config: raw_agent.runtime_config, // FIXME: make it actual implementaiton
        })
    }
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`SupervisorConfig`].
type AgentVariables = HashMap<String, Spec>;

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(try_from = "IntermediateEndSpec")]
pub struct EndSpec {
    description: String,
    #[serde(rename = "type")]
    pub type_: VariableType,
    pub required: bool,
    pub default: Option<TrivialValue>,
    /// The actual value that will be used by the agent. This will be either the user-provided value or, if not provided and not marked as [`required`], the default value.
    #[serde(skip)]
    pub final_value: Option<TrivialValue>,
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
    // #[serde(rename = "map[string]string")]
    // MapStringString,
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
        Ok(EndSpec {
            default: ies.default.map(|d| d.check_type(ies.type_)).transpose()?,
            final_value: None,
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
type NormalizedVariables = HashMap<String, EndSpec>;

fn normalize_agent_spec(spec: AgentVariables) -> Result<NormalizedVariables, AgentTypeError> {
    spec.into_iter().try_fold(HashMap::new(), |r, (k, v)| {
        let n_spec = inner_normalize(k, v);
        n_spec.iter().try_for_each(|(k, v)| {
            if v.default.is_none() && !v.required {
                return Err(AgentTypeError::MissingDefaultWithKey(k.clone()));
            }
            if let Some(default) = v.default.clone() {
                default.check_type(v.type_)?;
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
            restart_policy::{BackoffStrategyConfig, BackoffStrategyInner},
            runtime_config::{Args, Env, Executable},
        },
        supervisor_config::SupervisorConfig,
    };

    use super::*;
    use serde_yaml::Error;
    use std::{collections::HashMap as Map, time::Duration};

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
            on_host.executables[0].clone().path.template()
        );
        assert_eq!(
            "-c ${deployment.k8s.image}".to_string(),
            on_host.executables[0].clone().args.template()
        );

        // Restart restart policy values
        assert_eq!(
            BackoffStrategyConfig::Fixed(BackoffStrategyInner {
                backoff_delay_seconds: TemplateableValue::from_template("1".to_string()),
                max_retries: TemplateableValue::from_template("3".to_string()),
                last_retry_interval_seconds: TemplateableValue::from_template("30".to_string()),
            }),
            on_host.restart_policy.backoff_strategy
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
        };

        assert_eq!(
            expected_spec,
            given_agent
                .get_variables("description.name".to_string())
                .unwrap()
        );
    }

    #[test]
    fn test_replacer() {
        let exec = Executable {
            path: TemplateableValue::from_template("${bin}/otelcol".to_string()),
            args: TemplateableValue::from_template("--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}".to_string()),
            env: TemplateableValue::from_template("".to_string()),
        };

        let user_values = Map::from([
            ("bin".to_string(), TrivialValue::String("/etc".to_string())),
            (
                "deployment.on_host.verbose".to_string(),
                TrivialValue::String("true".to_string()),
            ),
            (
                "deployment.on_host.log_level".to_string(),
                TrivialValue::String("trace".to_string()),
            ),
        ]);

        let exec_actual = exec.template_with(user_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue{value: Some("/etc/otelcol".to_string()), template: "${bin}/otelcol".to_string()},
            args: TemplateableValue{value: Some(Args("--verbose true --logs trace".to_string())), template: "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}".to_string()},
            env: TemplateableValue{value: Some(Env("".to_string())), template: "".to_string()},
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: TemplateableValue::from_template("${bin}/otelcol".to_string()),
            args: TemplateableValue::from_template("--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string()),
            env: TemplateableValue::from_template("".to_string()),
        };

        let user_values = Map::from([
            ("bin".to_string(), TrivialValue::String("/etc".to_string())),
            (
                "deployment.on_host.verbose".to_string(),
                TrivialValue::String("true".to_string()),
            ),
        ]);

        let exec_actual = exec.template_with(user_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue{value: Some("/etc/otelcol".to_string()), template: "${bin}/otelcol".to_string()},
            args: TemplateableValue{value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}".to_string()},
            env: TemplateableValue{value: Some(Env("".to_string())), template: "".to_string()},
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
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config ${config}"
        env: ""
"#;

    const GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML: &str = r#"
config: | 
    license_key: abc123
    staging: true
"#;

    #[test]
    fn test_template_with_runtime_field() {
        let input_agent_type =
            serde_yaml::from_str::<FinalAgent>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();
        println!("Input: {:#?}", input_agent_type);

        let input_user_config =
            serde_yaml::from_str::<SupervisorConfig>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML)
                .unwrap();
        println!("Input: {:#?}", input_user_config);

        let actual = input_agent_type
            .template_with(input_user_config)
            .expect("Failed to template_with the AgentType's runtime_config field");

        println!("Output: {:#?}", actual);
    }
}
