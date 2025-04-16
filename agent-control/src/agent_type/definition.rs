//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Agent Control will be running.
//!
//! The reasoning behind this is that the Agent Control will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Agent Control will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use super::{
    agent_type_id::AgentTypeID,
    error::AgentTypeError,
    runtime_config::Runtime,
    templates::TEMPLATE_KEY_SEPARATOR,
    variable::definition::{VariableDefinition, VariableDefinitionTree},
};

use crate::agent_control::defaults::default_capabilities;
use crate::values::yaml_config::YAMLConfig;
use opamp_client::operation::capabilities::Capabilities;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::warn;

/// AgentTypeDefinition represents the definition of an [AgentType]. It defines the variables and runtime for any supported
/// environment.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTypeDefinition {
    #[serde(flatten)]
    pub agent_type_id: AgentTypeID,
    pub variables: AgentTypeVariables,
    #[serde(flatten)]
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
    pub agent_type_id: AgentTypeID,
    pub variables: VariableTree,
    pub runtime_config: Runtime,
    capabilities: Capabilities,
}

impl AgentType {
    pub fn new(metadata: AgentTypeID, variables: VariableTree, runtime_config: Runtime) -> Self {
        Self {
            agent_type_id: metadata,
            variables,
            runtime_config,
            capabilities: default_capabilities(), // TODO: can capabilities be set in AgentTypeDefinition?
        }
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
            warn!(%key, "unexpected variable in the configuration");
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
mod agent_type_validation_tests;
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::run::Environment;
    use crate::agent_type::runtime_config::Deployment;
    use crate::{
        agent_type::trivial_value::{FilePathWithContent, TrivialValue},
        sub_agent::effective_agents_assembler::build_agent_type,
    };
    use assert_matches::assert_matches;
    use serde_yaml::{Error, Number};
    use std::collections::HashMap as Map;

    impl AgentTypeDefinition {
        /// This helper returns an [AgentTypeDefinition] including only the provided metadata
        pub fn empty_with_metadata(metadata: AgentTypeID) -> Self {
            Self {
                agent_type_id: metadata,
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

    #[test]
    fn test_basic_agent_parsing() {
        let basic_agent = r#"
name: nrdot
namespace: newrelic
version: 0.0.1
variables: {}
deployment: 
  on_host: {}
"#;

        let agent: AgentTypeDefinition = serde_yaml::from_str(basic_agent).unwrap();

        assert_eq!("nrdot", agent.agent_type_id.name());
        assert_eq!("newrelic", agent.agent_type_id.namespace());
        assert_eq!("0.0.1", agent.agent_type_id.version().to_string());
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
