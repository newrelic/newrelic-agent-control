//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Agent Control will be running.
//!
//! The reasoning behind this is that the Agent Control will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Agent Control will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use super::{
    agent_type_id::AgentTypeID,
    error::AgentTypeError,
    runtime_config::Runtime,
    variable::{Variable, VariableDefinition, tree::Tree},
};

use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::namespace::Namespace;
use crate::{agent_type::variable::tree::VarTree, values::yaml_config::YAMLConfig};
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
    pub common: VariableDefinitionTree,
    #[serde(default)]
    pub k8s: VariableDefinitionTree,
    #[serde(default)]
    pub linux: VariableDefinitionTree,
    #[serde(default)]
    pub windows: VariableDefinitionTree,
}

/// Configuration of the Agent Type, contains identification metadata, a set of variables that can be adjusted, and rules of how to execute agents.
///
/// This is the final representation of the agent type once it has been parsed (first into a [`AgentTypeDefinition`]), and it is aware of the corresponding environment.
#[derive(Debug, PartialEq, Clone)]
pub struct AgentType {
    pub agent_type_id: AgentTypeID,
    pub variables: VariableTree,
    pub runtime_config: Runtime,
}

impl AgentType {
    pub fn new(metadata: AgentTypeID, variables: VariableTree, runtime_config: Runtime) -> Self {
        Self {
            agent_type_id: metadata,
            variables,
            runtime_config,
        }
    }
}

pub type VariableTree = VarTree<Variable>;

pub type VariableDefinitionTree = VarTree<VariableDefinition>;

impl VariableDefinitionTree {
    /// Returns the corresponding [VariableTree] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> VariableTree {
        let mapping = self
            .0
            .into_iter()
            .map(|(k, v)| (k, v.with_config(constraints)))
            .collect();
        VarTree(mapping)
    }
}

impl Tree<VariableDefinition> {
    /// Returns the corresponding [Tree<Variable>] according to the provided configuration.
    fn with_config(self, constraints: &VariableConstraints) -> Tree<Variable> {
        match self {
            Tree::End(v) => Tree::End(v.with_config(constraints)),
            Tree::Mapping(mapping) => {
                let x = mapping
                    .into_iter()
                    .map(|(k, v)| (k, v.with_config(constraints)))
                    .collect();
                Tree::Mapping(x)
            }
        }
    }
}

impl VariableTree {
    /// Returns a new [VariableTree] with the provided values assigned.
    pub fn fill_with_values(self, values: YAMLConfig) -> Result<Self, AgentTypeError> {
        let mut vars = self.0;
        update_specs(values.into(), &mut vars)?;
        Ok(Self(vars))
    }
}

fn update_specs(
    values: HashMap<String, serde_yaml::Value>,
    agent_vars: &mut HashMap<String, Tree<Variable>>,
) -> Result<(), AgentTypeError> {
    for (ref key, value) in values.into_iter() {
        let Some(spec) = agent_vars.get_mut(key) else {
            warn!(%key, "Unexpected variable in the configuration");
            continue;
        };

        match spec {
            Tree::End(e) => e.merge_with_yaml_value(value)?,
            Tree::Mapping(m) => {
                let v: HashMap<String, serde_yaml::Value> = serde_yaml::from_value(value)?;
                update_specs(v, m)?
            }
        }
    }
    Ok(())
}

/// Represents a normalized version of [VariableTree].
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
/// Will be converted to `system.logging.level` and can be used later in the AgentType_Meta part as `${nr-var:system.logging.level}`.
pub(crate) type Variables = HashMap<String, Variable>;

impl From<VariableTree> for Variables {
    fn from(value: VariableTree) -> Self {
        value.flatten()
    }
}

// TODO refactor Variables into a struct with methods
pub fn get_sub_agent_variable(variables: &Variables, variable_name: &str) -> Option<String> {
    let key = Namespace::SubAgent.namespaced_name(variable_name);
    variables
        .get(&key)
        .and_then(Variable::get_final_value)
        .map(|value| value.to_string())
}

#[cfg(test)]
mod agent_type_validation_tests;

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::run::Environment;
    use crate::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
    use crate::agent_type::runtime_config::Deployment;
    use crate::agent_type::variable::constraints::VariableConstraints;
    use crate::{
        agent_type::trivial_value::TrivialValue,
        sub_agent::effective_agents_assembler::build_agent_type,
    };
    use serde_yaml::{Error, Number};
    use std::collections::HashMap as Map;

    impl AgentTypeDefinition {
        /// This helper returns an [AgentTypeDefinition] including only the provided metadata
        pub fn empty_with_metadata(metadata: AgentTypeID) -> Self {
            Self {
                agent_type_id: metadata,
                variables: AgentTypeVariables {
                    common: VariableDefinitionTree::default(),
                    k8s: VariableDefinitionTree::default(),
                    linux: VariableDefinitionTree::default(),
                    windows: VariableDefinitionTree::default(),
                },
                runtime_config: Runtime {
                    deployment: Deployment {
                        windows: None,
                        linux: None,
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
            build_agent_type(definition, environment, &VariableConstraints::default()).unwrap()
        }

        /// Retrieve the `variables` field of the agent type at the specified key, if any.
        pub fn get_variable(self, path: String) -> Option<Variable> {
            self.variables.flatten().get(&path).cloned()
        }

        /// Fills the AgentType's variables with provided yaml values (helper for testing purposes).
        ///
        /// # Panics
        ///
        /// It will panic if the yaml values are not valid or there is any error filling the variables in.
        pub fn fill_variables(&self, yaml_values: &str) -> HashMap<String, Variable> {
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
  linux:
    health:
      interval: 3s
      initial_delay: 3s
      timeout: 10s
      http:
        path: /healthz
        port: 8080
    executables:
      - id: otelcol
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
  linux:
    executables:
      - id: otelcol
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
  linux: {}
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
        println!("{raw_agent_err:?}");
        assert_eq!(
            raw_agent_err.unwrap_err().to_string(),
            "missing field `variables` at line 2 column 1"
        );
    }

    #[test]
    fn test_normalize_agent_spec() {
        // create AgentSpec
        let given_agent =
            AgentType::build_for_testing(AGENT_GIVEN_YAML, &AGENT_CONTROL_MODE_ON_HOST);

        let expected_map: Map<String, Variable> = Map::from([(
            "description.name".to_string(),
            Variable::new_string(
                "Name of the agent".to_string(),
                false,
                Some("nrdot".to_string()),
                None,
            ),
        )]);

        // expect output to be the map
        assert_eq!(expected_map, given_agent.variables.clone().flatten());

        let expected_spec = Variable::new_string(
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
    config3:
      description: "Newrelic infra configuration yaml"
      type: map[string]yaml
      required: true
    status_server_port:
      description: "Newrelic infra health status port"
      type: number
      required: false
      default: 8003
deployment:
  linux:
    health:
      interval: 3s
      initial_delay: 3s
      timeout: 10s
      http:
        path: /v1/status
        port: "${nr-var:status_server_port}"
    executables:
      - id: newrelic-infra
        path: /usr/bin/newrelic-infra
        args: "--config ${nr-var:config} --config2 ${nr-var:config2}"
    packages:
      infra-agent:
        type: tar.gz
        download:
          oci:
            registry: ${nr-var:registry}
            repository: ${nr-var:repository}
            version: ${nr-var:version}
"#;

    const GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML: &str = r#"
unknown_variable: ignored
config3:
  log_level: trace
  forward: "true"
status_server_port: 8004
"#;

    #[test]
    fn test_fill_infra_agent_variables_in() {
        // When we fill the agent type variables with the corresponding values
        let input_agent_type =
            AgentType::build_for_testing(GIVEN_NEWRELIC_INFRA_YAML, &AGENT_CONTROL_MODE_ON_HOST);
        let filled_variables =
            input_agent_type.fill_variables(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML);

        // Then, we expect the corresponding final values.
        let expected_config_3 = TrivialValue::MapStringYaml(HashMap::from([
            ("log_level".to_string(), "trace".into()),
            ("forward".to_string(), "true".into()),
        ]));
        // Number
        let expected_status_server = TrivialValue::Number(Number::from(8004));

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
        variants:
          values: [fixed, linear]
        default: exponential
deployment:
  linux:
      executables:
        - id: echo
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
            AgentType::build_for_testing(AGENT_TYPE_WITH_VARIANTS, &AGENT_CONTROL_MODE_ON_HOST);

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
            r#"invalid value provided. Variants allowed: [fixed, linear]"#
        );

        // Default invalid variant is allowed
        let filled_variables_default = agent_type.fill_variables("");
        let var = filled_variables_default.get("restart_policy.type").unwrap();
        assert_eq!(
            "exponential".to_string(),
            var.get_final_value().unwrap().to_string()
        );
    }
}
