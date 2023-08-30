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

use super::supervisor_config::{
    validate_with_agent_type, NormalizedSupervisorConfig, SupervisorConfig,
};

const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
const TEMPLATE_BEGIN: &str = "${";
const TEMPLATE_END: char = '}';
pub const TEMPLATE_KEY_SEPARATOR: &str = ".";

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
        expected_type: SpecType,
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

    #[error("Missing default value for a non-required spec key")]
    MissingDefault,
    #[error("Missing default value for spec key `{0}`")]
    MissingDefaultWithKey(String),
    #[error("Invalid default value for spec key `{key}`: expected a {type_:?}")]
    InvalidDefaultForSpec { key: String, type_: SpecType },
}

pub(super) type AgentName = String;

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
    pub name: AgentName,
    namespace: String,
    version: String,
}

impl Display for AgentMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.namespace, self.name, self.version)
    }
}

#[derive(Debug, PartialEq, Clone, Default, Deserialize)]
#[serde(try_from = "RawAgent")]
pub struct Agent {
    #[serde(flatten)]
    pub metadata: AgentMetadata,
    pub variables: NormalizedVariables,
    pub runtime_config: RuntimeConfig,
}

impl Agent {
    pub fn get_variables(self, path: String) -> Option<EndSpec> {
        self.variables.get(&path).cloned()
    }

    pub fn populate(self, config: SupervisorConfig) -> Result<Self, AgentTypeError> {
        let normalized_config = NormalizedSupervisorConfig::from(config);
        let validated_conf = validate_with_agent_type(normalized_config, &self)?;

        let runtime_conf = self.runtime_config.template_with(validated_conf.clone())?;
        let mut spec = self.variables;

        validated_conf.into_iter().for_each(|(k, v)| {
            spec.entry(k).and_modify(|s| {
                s.final_value = Some(v);
            });
        });

        Ok(Agent {
            runtime_config: runtime_conf,
            variables: spec,
            ..self
        })
    }
}

impl TryFrom<RawAgent> for Agent {
    type Error = AgentTypeError;

    fn try_from(raw_agent: RawAgent) -> Result<Self, Self::Error> {
        Ok(Agent {
            variables: normalize_agent_spec(raw_agent.variables)?,
            metadata: raw_agent.metadata,
            runtime_config: raw_agent.runtime_config,
        })
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    Bool(bool),
    Number(N),
}

impl TrivialValue {
    pub fn check_type(self, type_: SpecType) -> Result<Self, AgentTypeError> {
        match (self.clone(), type_) {
            (TrivialValue::String(_), SpecType::String)
            | (TrivialValue::Bool(_), SpecType::Bool)
            | (TrivialValue::Number(_), SpecType::Number) => Ok(self),
            (TrivialValue::String(s), SpecType::File) => {
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
        }
    }
}

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

type AgentVariables = Map<String, Spec>;

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(try_from = "IntermediateEndSpec")]
pub struct EndSpec {
    description: String,
    #[serde(rename = "type")]
    pub type_: SpecType,
    pub required: bool,
    pub default: Option<TrivialValue>,
    #[serde(skip)]
    pub final_value: Option<TrivialValue>,
}

#[derive(Debug, PartialEq, Clone, Copy, Deserialize)]
pub enum SpecType {
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
    type_: SpecType,
    required: bool,
    default: Option<TrivialValue>,
}

impl TryFrom<IntermediateEndSpec> for EndSpec {
    type Error = AgentTypeError;

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
pub struct RuntimeConfig {
    pub deployment: Deployment,
}

impl Templateable for RuntimeConfig {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
        Ok(RuntimeConfig {
            deployment: self.deployment.template_with(kv)?,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
}

impl Templateable for Deployment {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
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

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    pub executables: Vec<Executable>,
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
}

impl Templateable for OnHost {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
        Ok(OnHost {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(kv.clone()))
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
const BACKOFF_MAX_RETRIES: usize = 20;
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
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Self, AgentTypeError> {
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
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Self, AgentTypeError> {
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
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Executable, AgentTypeError> {
        Ok(Executable {
            path: self.path.template_with(kv.clone())?,
            args: self.args.template_with(kv.clone())?,
            env: self.env.template_with(kv)?,
        })
    }
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<String, AgentTypeError> {
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
                Ok(re.replace(&r, replacement.to_string()).to_string())
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

type NormalizedVariables = Map<String, EndSpec>;

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
                type_: SpecType::String,
                required: false,
                default: Some(TrivialValue::String("nrdot".to_string())),
                final_value: None,
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.variables);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: SpecType::String,
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

        let user_values = Map::from([
            ("bin".to_string(), TrivialValue::String("/etc".to_string())),
            (
                "deployment.on_host.verbose".to_string(),
                TrivialValue::String("true".to_string()),
            ),
        ]);

        let exec_actual = exec.template_with(user_values).unwrap();

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
