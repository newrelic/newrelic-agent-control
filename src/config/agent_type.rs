//! This module contains the definitions of the Supervisor's Agent Type, which is the type of agent that the Supervisor will be running.
//!
//! The reasoning behind this is that the Supervisor will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Super Agent will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::populate`] for a flowchart of the dataflow that ends in the final, enriched structure.
use regex::Regex;
use serde::Deserialize;
use serde_with::serde_as;
use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
    io,
    time::Duration,
};
use thiserror::Error;

use crate::supervisor::restart::{Backoff, BackoffStrategy};

use super::supervisor_config::SupervisorConfig;

use std::fs;
use std::io::Write;

use uuid::Uuid;

/// Regex that extracts the template values from a string.
///
/// Example:
///
/// ```
/// use regex::Regex;
///
/// const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
/// let re = Regex::new(TEMPLATE_RE).unwrap();
/// let content = "Hello ${name.value}!";
///
/// let result = re.find_iter(content).map(|i| i.as_str()).collect::<Vec<_>>();
///
/// assert_eq!(result, vec!["${name.value}"]);
const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
const TEMPLATE_BEGIN: &str = "${";
const TEMPLATE_END: char = '}';
pub const TEMPLATE_KEY_SEPARATOR: &str = ".";

/// The different error types to be returned by operations involving the [`Agent`] type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("Missing required key in config: `{0}`")]
    MissingAgentKey(String),
    #[error(
        "Type mismatch while parsing. Expected type {expected_type:?}, got value {actual_value:?}"
    )]
    TypeMismatch {
        expected_type: VariableType,
        actual_value: TrivialValue,
    },
    #[error("Found unexpected keys in config: {0:?}")]
    UnexpectedKeysInConfig(Vec<String>),
    #[error("I/O error: `{0}`")]
    IOError(#[from] io::Error),
    #[error("Attempted to store an invalid path on a FilePathWithContent object")]
    InvalidFilePath,
    #[error("Missing required template key: `{0}`")]
    MissingTemplateKey(String),
    #[error("Map values must be of the same type")]
    InvalidMap,

    #[error("Missing default value for a non-required spec key")]
    MissingDefault,
    #[error("Missing default value for spec key `{0}`")]
    MissingDefaultWithKey(String),
    #[error("Invalid default value for spec key `{key}`: expected a {type_:?}")]
    InvalidDefaultForSpec { key: String, type_: VariableType },
}

#[derive(Debug, Deserialize)]
struct RawAgent {
    #[serde(flatten)]
    metadata: AgentMetadata,
    variables: AgentVariables,
    #[serde(default, flatten)]
    runtime_config: RuntimeConfig,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct AgentMetadata {
    pub name: String,
    namespace: String,
    version: String,
}

impl Display for AgentMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.namespace, self.name, self.version)
    }
}

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to start given agent binaries.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`RawAgent`]) having the spec field normalized.
///
/// See also [`RawAgent`] and its [`Agent::try_from`] implementation.
#[derive(Debug, PartialEq, Clone, Default, Deserialize)]
#[serde(try_from = "RawAgent")]
pub struct Agent {
    #[serde(flatten)]
    pub metadata: AgentMetadata,
    pub variables: NormalizedVariables,
    pub runtime_config: RuntimeConfig,
}

impl Agent {
    /// Retrieve the `variables` field of the agent type at the specified key, if any.
    pub fn get_variables(self, path: String) -> Option<EndSpec> {
        self.variables.get(&path).cloned()
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// Populate the [`RuntimeConfig`] object field of the [`Agent`] type with the user-provided config, which must abide by the agent type's defined [`AgentVariables`].
    ///
    /// This method will return an error if the user-provided config does not conform to the agent type's spec.
    ///
    /// The expected overall dataflow ending in `populate`, with the functions involved, is the following:
    ///
    /// ```mermaid
    /// flowchart
    ///     subgraph main
    ///     A[User] --> |provides| B["Agent Type (YAML)"]
    ///     B --> |"parses into (serde)"| C[RawAgent]
    ///     C --> |"normalize_agent_spec()"| D[Agent]
    ///     D --> G{"Agent::populate()"}
    ///     A --> |provides| E["Agent Config (YAML)"]
    ///     E --> |"parses into (serde)"| F[SupervisorConfig]
    ///     F --> G{"Agent::populate()"}
    ///     end
    ///     subgraph "Agent::populate()"
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
    pub fn populate(self, config: SupervisorConfig) -> Result<Self, AgentTypeError> {
        let config = config.normalize_with_agent_type(&self)?;

        let mut spec = self.variables;

        // modifies variables final value with the one defined in the SupervisorConfig
        spec.iter_mut().for_each(|(k, v)| {
            let defined_value = config.get_from_normalized(k);
            v.final_value = defined_value.or(v.default.clone());
        });

        let runtime_conf = self.runtime_config.template_with(&spec)?;

        let mut populated_agent = Agent {
            runtime_config: runtime_conf,
            variables: spec,
            ..self
        };
        populated_agent.write_files()?;
        Ok(populated_agent)
    }

    // write_files stores the content of each TrivialValue::File into the corresponding file
    fn write_files(&mut self) -> Result<(), AgentTypeError> {
        self.variables
            .values_mut()
            .try_for_each(|v| -> Result<(), AgentTypeError> {
                if let Some(TrivialValue::File(f)) = &mut v.final_value {
                    write_file(f)?
                } else if let Some(TrivialValue::Map(m)) = &mut v.final_value {
                    return m.clone().iter_mut().try_for_each(|(key, mut file)| {
                        if let TrivialValue::File(f) = &mut file {
                            write_file(f)?;
                            m.insert(key.to_string(),file.clone());
                        }
                        Ok(())
                    })
                }
                Ok(())
            })
    }
}

fn write_file(file: &mut FilePathWithContent) -> Result<(), io::Error> {
    const CONF_DIR: &str = "agentconfigs";
    // get current path
    let wd = std::env::current_dir()?;
    let dir = wd.join(CONF_DIR);
    if !dir.exists() {
        fs::create_dir(dir.as_path())?;
    }
    let uuid = Uuid::new_v4().to_string();
    let path = format!("{}/{}-config.yaml", dir.to_string_lossy(), uuid); // FIXME: PATH?
    let mut fs_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)?;

    writeln!(fs_file, "{}", file.content)?;
    file.path = path;
    // f.path = file
    //     .path()
    //     .to_str()
    //     .ok_or(AgentTypeError::InvalidFilePath)?
    //     .to_string();
    Ok(())
}

impl TryFrom<RawAgent> for Agent {
    type Error = AgentTypeError;
    /// Convert a [`RawAgent`] into an [`Agent`].
    ///
    /// This is where the `variables` field of the [`RawAgent`] is normalized into a [`NormalizedVariables`].
    fn try_from(raw_agent: RawAgent) -> Result<Self, Self::Error> {
        Ok(Agent {
            variables: normalize_agent_spec(raw_agent.variables)?,
            metadata: raw_agent.metadata,
            runtime_config: raw_agent.runtime_config,
        })
    }
}

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    Bool(bool),
    Number(N),
    Map(Map<String, TrivialValue>),
}

impl TrivialValue {
    /// Checks the `TrivialValue` against the given [`VariableType`], erroring if they do not match.
    ///
    /// This is also in charge of converting a `TrivialValue::String` into a `TrivialValue::File`, using the actual string as the file content, if the given [`VariableType`] is `VariableType::File`.
    pub fn check_type(self, type_: VariableType) -> Result<Self, AgentTypeError> {
        match (self.clone(), type_) {
            (TrivialValue::String(_), VariableType::String)
            | (TrivialValue::Bool(_), VariableType::Bool)
            | (TrivialValue::File(_), VariableType::File)
            | (TrivialValue::Number(_), VariableType::Number) => Ok(self),
            (TrivialValue::Map(m), VariableType::MapStringString) => {
                if !m.iter().all(|(_, v)| matches!(v, TrivialValue::String(_))) {
                    return Err(AgentTypeError::InvalidMap);
                }
                Ok(self)
            }
            (TrivialValue::Map(m), VariableType::MapStringFile) => {
                if !m.iter().all(|(_, v)|
                    matches!(v, TrivialValue::String(_))
                ) {
                    return Err(AgentTypeError::InvalidMap);
                }

                let mut final_map = Map::new();
                m.iter().for_each(|(k, mut v)| {
                    final_map.insert(k.clone(), TrivialValue::File(FilePathWithContent::new(v.to_string())));
                });
                Ok(TrivialValue::Map(final_map))
            }
            (TrivialValue::String(s), VariableType::File) => {
                Ok(TrivialValue::File(FilePathWithContent::new(s)))
            }
            (v, t) => Err(AgentTypeError::TypeMismatch {
                expected_type: t,
                actual_value: v,
            }),
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{}", s),
            TrivialValue::File(file) => write!(f, "{}", file.path),
            TrivialValue::Bool(b) => write!(f, "{}", b),
            TrivialValue::Number(n) => write!(f, "{}", n),
            TrivialValue::Map(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
        }
    }
}

/// Represents a file path and its content.
#[derive(Debug, PartialEq, Default, Clone, Deserialize)]
pub struct FilePathWithContent {
    #[serde(skip)]
    pub path: String,
    #[serde(flatten)]
    pub content: String,
}

impl FilePathWithContent {
    pub fn new(content: String) -> Self {
        FilePathWithContent {
            content,
            ..Default::default()
        }
    }
}

/// Represents a numeric value, which can be either a positive integer, a negative integer or a float.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum N {
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

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`SupervisorConfig`].
type AgentVariables = Map<String, Spec>;

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

/// Strict structure that describes how to start a given agent with all needed binaries, arguments, env, etc.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct RuntimeConfig {
    pub deployment: Deployment,
}

impl Templateable for RuntimeConfig {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(RuntimeConfig {
            deployment: self.deployment.template_with(variables)?,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
}

impl Templateable for Deployment {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        /*
        `self.on_host` has type `Option<OnHost>`

        let t = self.on_host.map(|o| o.template_with(kv)); `t` has type `Option<Result<OnHost, AgentTypeError>>`

        Let's visit all the possibilities of `t`.
        When I do `t.transpose()`, which takes an Option<Result<_,_>> and returns a Result<Option<_>,_>, this is what happens:

        ```
        match t {
            None => Ok(None),
            Some(Ok(on_host)) => Ok(Some(on_host)),
            Some(Err(e)) => Err(e),
        }
        ```

        In words:
        - None will be mapped to Ok(None).
        - Some(Ok(_)) will be mapped to Ok(Some(_)).
        - Some(Err(_)) will be mapped to Err(_).

        With `?` I get rid of the original Result<_,_> wrapper type and get the Option<_> (or else the error bubbles up if it contained the Err(_) variant). Then I am able to store that Option<_>, be it None or Some(_), back into the Deployment object which contains the Option<_> field.
         */

        let oh = self.on_host.map(|oh| oh.template_with(kv)).transpose()?;
        Ok(Deployment { on_host: oh })
    }
}

/// The definition for an on-host supervisor.
///
/// It contains the instructions of what are the agent binaries, command-line arguments, the environment variables passed to it and the restart policy of the supervisor.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    pub executables: Vec<Executable>,
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
}

impl Templateable for OnHost {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(OnHost {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(kv))
                .collect::<Result<Vec<Executable>, AgentTypeError>>()?,
            ..Default::default()
        })
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
    #[serde(default)]
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyConfig {
    None,
    Fixed(BackoffStrategyInner),
    Linear(BackoffStrategyInner),
    Exponential(BackoffStrategyInner),
}

/* FIXME: This is not TEMPLATEABLE for the moment, we need to think what would be the strategy here and clarify:

1. If we perform replacement with the template but the values are not of the expected type, what happens?
2. Should we use an intermediate type with all the end nodes as `String` so we can perform the replacement?
  - Add a sanitize or a fallible conversion from the raw intermediate type into into the end type?


*/

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
const BACKOFF_DELAY: Duration = Duration::from_secs(2);
const BACKOFF_MAX_RETRIES: usize = 0;
const BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(600);

#[serde_as]
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyInner {
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub backoff_delay_seconds: Duration,
    pub max_retries: usize,
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub last_retry_interval_seconds: Duration,
}

impl From<&BackoffStrategyConfig> for BackoffStrategy {
    fn from(value: &BackoffStrategyConfig) -> Self {
        match value {
            BackoffStrategyConfig::Fixed(inner) => {
                BackoffStrategy::Fixed(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::Linear(inner) => {
                BackoffStrategy::Linear(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::Exponential(inner) => {
                BackoffStrategy::Exponential(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::None => BackoffStrategy::None,
        }
    }
}

impl Default for BackoffStrategyConfig {
    fn default() -> Self {
        Self::Linear(BackoffStrategyInner::default())
    }
}

impl Default for BackoffStrategyInner {
    fn default() -> Self {
        Self {
            backoff_delay_seconds: BACKOFF_DELAY,
            max_retries: BACKOFF_MAX_RETRIES,
            last_retry_interval_seconds: BACKOFF_LAST_RETRY_INTERVAL,
        }
    }
}

fn realize_backoff_config(i: &BackoffStrategyInner) -> Backoff {
    Backoff::new()
        .with_initial_delay(i.backoff_delay_seconds)
        .with_max_retries(i.max_retries)
        .with_last_retry_interval(i.last_retry_interval_seconds)
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Executable {
    pub path: String,
    #[serde(default)]
    pub args: Args,
    #[serde(default)]
    pub env: Env,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(String);

impl Templateable for Args {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Args(self.0.template_with(kv)?))
    }
}

impl Args {
    pub fn into_vector(self) -> Vec<String> {
        self.0.split_whitespace().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Env(String);

impl Templateable for Env {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Env(self.0.template_with(kv)?))
    }
}

impl Env {
    pub fn into_map(self) -> Map<String, String> {
        self.0
            .split_whitespace()
            .map(|s| {
                // FIXME: Non-existing '=' on input??
                s.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .unwrap()
            })
            .collect()
    }
}

trait Templateable {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, kv: &NormalizedVariables) -> Result<Executable, AgentTypeError> {
        Ok(Executable {
            path: self.path.template_with(kv)?,
            args: self.args.template_with(kv)?,
            env: self.env.template_with(kv)?,
        })
    }
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, kv: &NormalizedVariables) -> Result<String, AgentTypeError> {
        let re = Regex::new(TEMPLATE_RE).unwrap();

        let result = re
            .find_iter(&self.clone())
            .map(|i| i.as_str())
            .try_fold(self, |r, i| {
                let trimmed_s = i
                    .trim_start_matches(TEMPLATE_BEGIN)
                    .trim_end_matches(TEMPLATE_END);
                if !kv.contains_key(trimmed_s) {
                    return Err(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()));
                }
                let replacement = &kv[trimmed_s];
                Ok(re
                    .replace(
                        &r,
                        replacement
                            .final_value
                            .as_ref()
                            .or(replacement.default.as_ref())
                            .ok_or(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()))?
                            .to_string(),
                    )
                    .to_string())
            });
        result
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
    SpecMapping(Map<String, Spec>),
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
pub(crate) type NormalizedVariables = Map<String, EndSpec>;

fn normalize_agent_spec(spec: AgentVariables) -> Result<NormalizedVariables, AgentTypeError> {
    spec.into_iter().try_fold(Map::new(), |r, (k, v)| {
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
    let mut result = Map::new();
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
    use crate::config::supervisor_config::SupervisorConfig;

    use super::*;
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

    #[test]
    fn test_basic_parsing() {
        let agent: Agent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        assert_eq!("nrdot", agent.metadata.name);
        assert_eq!("newrelic", agent.metadata.namespace);
        assert_eq!("0.1.0", agent.metadata.version);

        let on_host = agent.runtime_config.deployment.on_host.clone().unwrap();

        assert_eq!("${bin}/otelcol", on_host.executables[0].path);
        assert_eq!(
            Args("-c ${deployment.k8s.image}".to_string()),
            on_host.executables[0].args
        );

        // Resrtart restart policy values
        assert_eq!(
            BackoffStrategyConfig::Fixed(BackoffStrategyInner {
                backoff_delay_seconds: Duration::from_secs(1),
                max_retries: 3,
                last_retry_interval_seconds: Duration::from_secs(30),
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

        let given_agent: Agent = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

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
            path: "${bin}/otelcol".to_string(),
            args: Args(
                "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: Env("".to_string()),
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
                },
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: "/etc/otelcol".to_string(),
            args: Args("--verbose true --logs trace".to_string()),
            env: Env("".to_string()),
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: "${bin}/otelcol".to_string(),
            args: Args("--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}"
                .to_string()),
            env: Env("".to_string()),
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
                },
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: "/etc/otelcol".to_string(),
            args: Args("--verbose true --verbose_again true".to_string()),
            env: Env("".to_string()),
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
  config2:
    description: "Newrelic infra configuration yaml"
    type: file
    required: false
    default: |
        license_key: abc123
        staging: true
  config3:
    description: "Newrelic infra configuration yaml"
    type: map[string]string
    required: true
  integrations:
    description: "Newrelic integrations configuration yamls"
    type: map[string]file
    required: true
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
  kafka: |
    strategy: bootstrap
  redis: |
    user: redis
config: | 
    license_key: abc123
    staging: true
"#;

    #[test]
    fn test_populate_runtime_field() {
        let input_agent_type = serde_yaml::from_str::<Agent>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();

        println!("Input: {:#?}", input_agent_type);

        let input_user_config =
            serde_yaml::from_str::<SupervisorConfig>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML)
                .unwrap();
        println!("Input: {:#?}", input_user_config);

        let actual = input_agent_type
            .populate(input_user_config)
            .expect("Failed to populate the AgentType's runtime_config field");

        println!("Output: {:#?}", actual);
    }
}
