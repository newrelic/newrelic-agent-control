//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Agent Control will be running.
//!
//! The reasoning behind this is that the Agent Control will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Agent Control will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use super::{
    agent_metadata::AgentMetadata,
    error::AgentTypeError,
    restart_policy::{BackoffDelay, BackoffLastRetryInterval, MaxRetries},
    runtime_config::{Args, Runtime},
    runtime_config_templates::{Templateable, TEMPLATE_KEY_SEPARATOR},
    variable::definition::{VariableDefinition, VariableDefinitionTree},
};
use crate::agent_control::config::AgentTypeFQN;
use crate::agent_control::defaults::default_capabilities;
use crate::values::yaml_config::YAMLConfig;
use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Deserializer};
use std::{collections::HashMap, str::FromStr};
use tracing::warn;

/// AgentTypeDefinition represents the definition of an [AgentType]. It defines the variables and runtime for any supported
/// environment.
#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct AgentTypeDefinition {
    #[serde(flatten)]
    pub metadata: AgentMetadata,
    pub variables: AgentTypeVariables,
    #[serde(default, flatten)]
    pub runtime_config: Runtime,
}

/// Contains the variable definitions that can be defined in an [AgentTypeDefinition].
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct AgentTypeVariables {
    #[serde(default)]
    pub common: VariableTree,
    #[serde(default)]
    pub k8s: VariableTree,
    #[serde(default)]
    pub on_host: VariableTree,
}

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to execute agents.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`AgentTypeDefinition`]), and it is aware of the corresponding environment.
#[derive(Debug, PartialEq, Clone)]
pub struct AgentType {
    pub metadata: AgentMetadata,
    pub variables: VariableTree,
    pub runtime_config: Runtime,
    capabilities: Capabilities,
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct TemplateableValue<T> {
    pub(super) value: Option<T>,
    pub(super) template: String,
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

impl Templateable for TemplateableValue<Args> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        Ok(Self {
            template: self.template,
            value: Some(Args(templated_string)),
        })
    }
}

impl Templateable for TemplateableValue<BackoffDelay> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            BackoffDelay::default()
        } else {
            // Attempt to parse a simple number as seconds
            duration_str::parse(&templated_string)
                .map(BackoffDelay::from)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<BackoffLastRetryInterval> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            BackoffLastRetryInterval::default()
        } else {
            // Attempt to parse a simple number as seconds
            duration_str::parse(&templated_string)
                .map(BackoffLastRetryInterval::from)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<MaxRetries> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            MaxRetries::default()
        } else {
            templated_string
                .parse::<usize>()
                .map(MaxRetries::from)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl AgentType {
    pub fn new(metadata: AgentMetadata, variables: VariableTree, runtime_config: Runtime) -> Self {
        Self {
            metadata,
            variables,
            runtime_config,
            capabilities: default_capabilities(), // TODO: can capabilities be set in AgentTypeDefinition?
        }
    }

    // TODO: AgentTypeFQN should not exist and always use the metadata display.
    pub fn agent_type(&self) -> AgentTypeFQN {
        self.metadata
            .to_string()
            .as_str()
            .try_into()
            .expect("incorrect AgentType metadata")
    }

    pub fn get_variables(&self) -> Variables {
        self.variables.clone().flatten()
    }

    pub fn get_capabilities(&self) -> Capabilities {
        self.capabilities
    }
}

/// Flexible tree-like structure that contains variables definitions, that can later be changed by the end user via [`YAMLConfig`].
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct VariableTree(pub(crate) HashMap<String, VariableDefinitionTree>);

impl VariableTree {
    pub fn flatten(self) -> HashMap<String, VariableDefinition> {
        self.0
            .into_iter()
            .flat_map(|(k, v)| inner_flatten(k, v))
            .collect()
    }

    /// Returns a new [VariableTree] with the provided values assigned.
    pub fn fill_with_values(self, values: YAMLConfig) -> Result<Self, AgentTypeError> {
        let mut vars = self.0.clone();
        update_specs(values.into(), &mut vars)?;
        Ok(Self(vars))
    }

    /// Merges the current [VariableTree] with another, returning an error if any key overlaps.
    pub fn merge(self, variables: Self) -> Result<Self, AgentTypeError> {
        Ok(Self(
            Self::merge_inner(&self.0, &variables.0)
                .map_err(AgentTypeError::ConflictingVariableDefinition)?,
        ))
    }

    /// Merges recursively two inner hashmaps if there is no conflicting key.
    ///
    /// # Errors
    ///
    /// This function will return an String error containing the full path of the first conflicting key if any
    /// [VariableDefinitionTree::End] overlaps.
    fn merge_inner(
        a: &HashMap<String, VariableDefinitionTree>,
        b: &HashMap<String, VariableDefinitionTree>,
    ) -> Result<HashMap<String, VariableDefinitionTree>, String> {
        let mut merged = a.clone();
        for (key, value) in b {
            match (merged.get(key), value) {
                // Include the value when its key doesn't overlap.
                (None, _) => {
                    merged.insert(key.into(), value.clone());
                }
                // Merge overlapping mappings.
                (
                    Some(VariableDefinitionTree::Mapping(inner_a)),
                    VariableDefinitionTree::Mapping(inner_b),
                ) => {
                    let merged_inner = Self::merge_inner(inner_a, inner_b)
                        .map_err(|err| format!("{key}.{err}"))?;
                    merged.insert(key.clone(), VariableDefinitionTree::Mapping(merged_inner));
                }
                // Any other option implies an overlapping end (conflicting key).
                (Some(_), _) => return Err(key.into()),
            }
        }
        Ok(merged)
    }
}

fn update_specs(
    values: HashMap<String, serde_yaml::Value>,
    agent_vars: &mut HashMap<String, VariableDefinitionTree>,
) -> Result<(), AgentTypeError> {
    for (ref key, value) in values.into_iter() {
        let Some(spec) = agent_vars.get_mut(key) else {
            warn!(%key, %value, "Unexpected variable in the configuration");
            continue;
        };

        match spec {
            VariableDefinitionTree::End(e) => e.merge_with_yaml_value(value)?,
            VariableDefinitionTree::Mapping(m) => {
                let v: HashMap<String, serde_yaml::Value> = serde_yaml::from_value(value)?;
                update_specs(v, m)?
            }
        }
    }
    Ok(())
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
///   common:
///     system:
///       logging:
///         level:
///           description: "Logging level"
///           type: string
///           required: false
///           default: info
/// ```
///
/// Will be converted to `system.logging.level` and can be used later in the AgentType_Meta part as `${nr-var:system.logging.level}`.
pub(crate) type Variables = HashMap<String, VariableDefinition>;

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::runtime_config::Deployment;
    use crate::{
        agent_type::{
            environment::Environment,
            restart_policy::{BackoffStrategyConfig, BackoffStrategyType, RestartPolicyConfig},
            runtime_config::{Env, Executable},
            trivial_value::{FilePathWithContent, TrivialValue},
        },
        sub_agent::effective_agents_assembler::build_agent_type,
    };
    use assert_matches::assert_matches;
    use serde_yaml::{Error, Number};
    use std::collections::HashMap as Map;

    impl AgentTypeDefinition {
        /// This helper returns an [AgentTypeDefinition] including only the provided metadata
        pub fn empty_with_metadata(metadata: AgentMetadata) -> Self {
            Self {
                metadata,
                variables: AgentTypeVariables {
                    common: VariableTree::default(),
                    k8s: VariableTree::default(),
                    on_host: VariableTree::default(),
                },
                runtime_config: Runtime {
                    deployment: Deployment {
                        on_host: None,
                        k8s: None,
                    },
                },
            }
        }
    }

    impl AgentType {
        /// Builds a testing agent-type given the yaml definitions and the environment.
        ///
        /// # Panics
        ///
        /// The function will panic if the definition is not valid or not compatible with the environment.
        pub fn build_for_testing(yaml_definition: &str, environment: &Environment) -> Self {
            let definition = serde_yaml::from_str::<AgentTypeDefinition>(yaml_definition).unwrap();
            build_agent_type(definition, environment).unwrap()
        }

        pub fn set_capabilities(&mut self, capabilities: Capabilities) {
            self.capabilities = capabilities
        }

        /// Retrieve the `variables` field of the agent type at the specified key, if any.
        pub fn get_variable(self, path: String) -> Option<VariableDefinition> {
            self.variables.flatten().get(&path).cloned()
        }

        /// Fills the AgentType's variables with provided yaml values (helper for testing purposes).
        ///
        /// # Panics
        ///
        /// It will panic if the yaml values are not valid or there is any error filling the variables in.
        pub fn fill_variables(&self, yaml_values: &str) -> HashMap<String, VariableDefinition> {
            let values = serde_yaml::from_str::<YAMLConfig>(yaml_values).unwrap();
            self.variables
                .clone()
                .fill_with_values(values)
                .unwrap()
                .flatten()
        }
    }

    pub const AGENT_GIVEN_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.0.1
variables:
  common:
    description:
      name:
        description: "Name of the agent"
        type: string
        required: false
        default: nrdot
deployment:
  on_host:
    health:
      interval: 3s
      timeout: 10s
      http:
        path: /healthz
        port: 8080
    executable:
      path: ${nr-var:bin}/otelcol
      args: "-c ${nr-var:deployment.k8s.image}"
      restart_policy:
        backoff_strategy:
          type: fixed
          backoff_delay: 1s
          max_retries: 3
          last_retry_interval: 30s
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
    executable:
      path: ${nr-var:bin}/otelcol
      args: "-c ${nr-var:deployment.k8s.image}"
"#;

    pub const RESTART_POLICY_OMITTED_FIELDS_YAML: &str = r#"
restart_policy:
  backoff_strategy:
    type: linear
"#;

    #[test]
    fn test_basic_agent_parsing() {
        let agent: AgentTypeDefinition = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        assert_eq!("nrdot", agent.metadata.name);
        assert_eq!("newrelic", agent.metadata.namespace);
        assert_eq!("0.0.1", agent.metadata.version.to_string());

        let on_host = agent.runtime_config.deployment.on_host.clone().unwrap();

        assert_eq!(
            "${nr-var:bin}/otelcol",
            on_host.executable.clone().unwrap().path.template
        );
        assert_eq!(
            "-c ${nr-var:deployment.k8s.image}".to_string(),
            on_host.executable.clone().unwrap().args.template
        );

        // Restart policy values
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("fixed".to_string()),
                backoff_delay: TemplateableValue::from_template("1s".to_string()),
                max_retries: TemplateableValue::from_template("3".to_string()),
                last_retry_interval: TemplateableValue::from_template("30s".to_string()),
            },
            on_host.executable.unwrap().restart_policy.backoff_strategy
        );
    }

    #[test]
    fn test_no_executables() {
        const AGENT_TYPE_NO_EXECUTABLES: &str = r#"
name: no-exec
namespace: newrelic
version: 0.0.1
variables: {}
deployment:
  on_host: {}
"#;

        let agent: AgentTypeDefinition = serde_yaml::from_str(AGENT_TYPE_NO_EXECUTABLES).unwrap();

        assert_eq!("no-exec", agent.metadata.name);
        assert_eq!("newrelic", agent.metadata.namespace);
        assert_eq!("0.0.1", agent.metadata.version.to_string());
        assert!(agent
            .runtime_config
            .deployment
            .on_host
            .unwrap()
            .executable
            .is_none());
    }

    #[test]
    fn test_agent_parsing_omitted_fields_use_defaults() {
        let backoff_strategy: BackoffStrategyConfig =
            serde_yaml::from_str(RESTART_POLICY_OMITTED_FIELDS_YAML).unwrap();

        // Restart policy values
        assert_eq!(BackoffStrategyConfig::default(), backoff_strategy);
    }

    #[test]
    fn test_bad_parsing() {
        let raw_agent_err: Result<AgentTypeDefinition, Error> =
            serde_yaml::from_str(AGENT_GIVEN_BAD_YAML);

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
        let given_agent = AgentType::build_for_testing(AGENT_GIVEN_YAML, &Environment::OnHost);

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
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: TemplateableValue::from_template(
                "--config ${nr-var:config} --plugin_dir ${nr-var:integrations} --verbose ${nr-var:deployment.on_host.verbose} --logs ${nr-var:deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template("${nr-var:backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::from_template("${nr-var:backoff.delay}".to_string()),
                    max_retries: TemplateableValue::from_template("${nr-var:backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::from_template(
                        "${nr-var:backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: Vec::default(),
            },
        };

        let normalized_values = Map::from([
            (
                "nr-var:bin".to_string(),
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:config".to_string(),
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
                "nr-var:integrations".to_string(),
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
                "nr-var:deployment.on_host.verbose".to_string(),
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:deployment.on_host.log_level".to_string(),
                VariableDefinition::new(
                    "log_level".to_string(),
                    true,
                    None,
                    Some("trace".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("exponential".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
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
                template: "${nr-var:bin}/otelcol".to_string(),
            },
            args: TemplateableValue {
                value: Some(Args("--config config_path --plugin_dir integration_path --verbose true --logs trace".to_string())),
                template:
                "--config ${nr-var:config} --plugin_dir ${nr-var:integrations} --verbose ${nr-var:deployment.on_host.verbose} --logs ${nr-var:deployment.on_host.log_level}"
                    .to_string(),
            },
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Exponential),
                        template: "${nr-var:backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDelay::from_secs(10)),
                        template: "${nr-var:backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30.into()),
                        template: "${nr-var:backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffLastRetryInterval::from_secs(300)),
                        template: "${nr-var:backoff.interval}".to_string(),
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
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: TemplateableValue::from_template("--verbose ${nr-var:deployment.on_host.verbose} --verbose_again ${nr-var:deployment.on_host.verbose}".to_string()),
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template(
                        "${nr-var:backoff.type}"
                            .to_string(),
                    ),
                    backoff_delay: TemplateableValue::from_template(
                        "${nr-var:backoff.delay}"
                            .to_string(),
                    ),
                    max_retries: TemplateableValue::from_template(
                        "${nr-var:backoff.retries}"
                            .to_string(),
                    ),
                    last_retry_interval: TemplateableValue::from_template(
                        "${nr-var:backoff.interval}"
                            .to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let normalized_values = Map::from([
            (
                "nr-var:bin".to_string(),
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:deployment.on_host.verbose".to_string(),
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("linear".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
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
            path: TemplateableValue { value: Some("/etc/otelcol".to_string()), template: "${nr-var:bin}/otelcol".to_string() },
            args: TemplateableValue { value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${nr-var:deployment.on_host.verbose} --verbose_again ${nr-var:deployment.on_host.verbose}".to_string() },
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Linear),
                        template: "${nr-var:backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDelay::from_secs(10)),
                        template: "${nr-var:backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30.into()),
                        template: "${nr-var:backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffLastRetryInterval::from_secs(300)),
                        template: "${nr-var:backoff.interval}".to_string(),
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
  common:
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
    status_server_port:
      description: "Newrelic infra health status port"
      type: number
      required: false
      default: 8003
deployment:
  on_host:
    health:
      interval: 3s
      timeout: 10s
      http:
        path: /v1/status
        port: "${nr-var:status_server_port}"
    executable:
      path: /usr/bin/newrelic-infra
      args: "--config ${nr-var:config} --config2 ${nr-var:config2}"
"#;

    const GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML: &str = r#"
unknown_variable: ignored
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
status_server_port: 8004
"#;

    #[test]
    fn test_fill_infra_agent_variables_in() {
        // When we fill the agent type variables with the corresponding values
        let input_agent_type =
            AgentType::build_for_testing(GIVEN_NEWRELIC_INFRA_YAML, &Environment::OnHost);
        let filled_variables =
            input_agent_type.fill_variables(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML);

        // Then, we expect the corresponding final values.
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
        // Number
        let expected_status_server: TrivialValue = Number::from(8004).into();

        assert_eq!(
            expected_config_3,
            filled_variables
                .get("config3")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_config_2,
            filled_variables
                .get("config2")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_config,
            filled_variables
                .get("config")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_integrations,
            filled_variables
                .get("integrations")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert_eq!(
            expected_status_server,
            filled_variables
                .get("status_server_port")
                .unwrap()
                .get_final_value()
                .as_ref()
                .unwrap()
                .clone()
        );
        assert!(!filled_variables.contains_key("unknown_variable"))
    }

    const AGENT_TYPE_WITH_VARIANTS: &str = r#"
name: variant-values
namespace: newrelic
version: 0.0.1
variables:
  common:
    restart_policy:
      type:
        description: "restart policy type"
        type: string
        required: false
        variants: [fixed, linear]
        default: exponential
deployment:
  on_host:
      executable:
        path: /bin/echo
        args: "${nr-var:restart_policy.type}"
"#;

    const VALUES_VALID_VARIANT: &str = r#"
restart_policy:
    type: fixed
"#;

    const VALUES_INVALID_VARIANT: &str = r#"
restart_policy:
    type: random
"#;

    #[test]
    fn test_variables_with_variants() {
        let agent_type =
            AgentType::build_for_testing(AGENT_TYPE_WITH_VARIANTS, &Environment::OnHost);

        // Valid variant
        let filled_variables = agent_type.fill_variables(VALUES_VALID_VARIANT);

        let var = filled_variables.get("restart_policy.type").unwrap();
        assert_eq!(
            "fixed".to_string(),
            var.get_final_value().unwrap().to_string()
        );

        // Invalid variant
        let invalid_values: YAMLConfig =
            serde_yaml::from_str(VALUES_INVALID_VARIANT).expect("Failed to parse user config");
        let filled_variables_result = agent_type
            .variables
            .clone()
            .fill_with_values(invalid_values);
        assert!(filled_variables_result.is_err());
        assert_eq!(
            filled_variables_result.unwrap_err().to_string(),
            r#"Invalid variant provided as a value: `"random"`. Variants allowed: ["\"fixed\"", "\"linear\""]"#
        );

        // Default invalid variant is allowed
        let filled_variables_default = agent_type.fill_variables("");
        let var = filled_variables_default.get("restart_policy.type").unwrap();
        assert_eq!(
            "exponential".to_string(),
            var.get_final_value().unwrap().to_string()
        );
    }

    #[test]
    fn test_merge_variable_tree() {
        let a = r#"
config:
  general:
    description: "General"
    type: string
    required: true
common:
  description: "Common"
  type: string
  required: true
"#;
        let b = r#"
config:
  specific:
    description: "Specific"
    type: string
    required: true
env:
  key:
    description: "key"
    type: string
    required: true
"#;
        let expected = r#"
config:
  general:
    description: "General"
    type: string
    required: true
  specific:
    description: "Specific"
    type: string
    required: true
common:
  description: "Common"
  type: string
  required: true
env:
  key:
    description: "key"
    type: string
    required: true
"#;
        let a: VariableTree = serde_yaml::from_str(a).unwrap();
        let b: VariableTree = serde_yaml::from_str(b).unwrap();
        let expected: VariableTree = serde_yaml::from_str(expected).unwrap();

        assert_eq!(expected, a.merge(b).unwrap());
    }

    #[test]
    fn test_merge_variable_tree_errors() {
        struct TestCase {
            name: &'static str,
            a: &'static str,
            b: &'static str,
            conflicting_key: &'static str,
        }
        impl TestCase {
            fn run(&self) {
                let a: VariableTree = serde_yaml::from_str(self.a).unwrap();
                let b: VariableTree = serde_yaml::from_str(self.b).unwrap();
                let err = a.merge(b).unwrap_err();
                assert_matches!(err, AgentTypeError::ConflictingVariableDefinition(k) => {
                    assert_eq!(self.conflicting_key, k, "{}", self.name);
                })
            }
        }
        let test_cases = vec![
            TestCase {
                name: "Conflicting leaves",
                a: r#"
config:
  general:
    description: "General"
    type: string
    required: true
"#,
                b: r#"
config:
  general:
    description: "General"
    type: string
    required: true
"#,
                conflicting_key: "config.general",
            },
            TestCase {
                name: "Conflicting branch with leave",
                a: r#"
var:
  nested:
    description: "Nested"
    type: string
    required: true
"#,
                b: r#"
var:
  description: "var"
  type: string
  required: true
"#,
                conflicting_key: "var",
            },
            TestCase {
                name: "Conflicting leave with branch",
                a: r#"
var:
  description: "var"
  type: string
  required: true
"#,
                b: r#"
var:
  nested:
    description: "Nested"
    type: string
    required: true
"#,
                conflicting_key: "var",
            },
            TestCase {
                name: "Conflicting branch and leave nested",
                a: r#"
var:
  several:
    nested:
      levels:
        description: "levels"
        type: string
        required: true
"#,
                b: r#"
var:
  several:
    nested:
      description: "nested"
      type: string
      required: true
"#,
                conflicting_key: "var.several.nested",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
