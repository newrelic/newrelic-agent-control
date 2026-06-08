//! This module contains the definitions of the SubAgent's Agent Type, which is the type of agent that the Agent Control will be running.
//!
//! The reasoning behind this is that the Agent Control will be able to run different types of agents, and each type of agent will have its own configuration. Supporting generic agent functionalities, the user can both define its own agent types and provide a config that implement this agent type, and the New Relic Agent Control will spawn a Supervisor which will be able to run it.
//!
//! See [`Agent::template_with`] for a flowchart of the dataflow that ends in the final, enriched structure.

use super::{
    agent_type_id::AgentTypeID,
    error::AgentTypeError,
    runtime_config::{Deployment, Runtime},
    variable::{Variable, VariableDefinition, tree::Tree},
};
use crate::environment::Environment;

use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::OnHost;
use crate::agent_type::runtime_config::on_host::rendered::RenderedPackages;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::namespace::Namespace;
use crate::package::oci::package_manager::get_package_path;
use crate::{agent_control::agent_id::AgentID, package::manager::PackageData};
use crate::{agent_type::variable::tree::VarTree, values::yaml_config::YAMLConfig};
use serde::{Deserialize, de::Error as _};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use tracing::{debug, warn};

/// The agent type as parsed from a YAML file. Variables are still in their raw
/// `VariableDefinitionTree` form. Apply [AgentTypeDefinition::with_constraints] to materialize
/// the [AgentType] used for rendering.
///
/// Deserialization is **platform-driven**: the custom Deserialize first reads the agent-type id
/// (which carries the `Environment`), and then dispatches the `deployment` block to either
/// [OnHost] or [K8s]. This way each variant gets its own native `Deserialize` implementation,
/// with precise error messages.
#[derive(Debug, PartialEq, Clone)]
pub struct AgentTypeDefinition {
    pub metadata: AgentTypeMetadata,
    pub variables: VariableDefinitionTree,
    pub runtime_config: Runtime,
}

impl<'de> Deserialize<'de> for AgentTypeDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(flatten)]
            metadata: AgentTypeMetadata,
            #[serde(default)]
            variables: VariableDefinitionTree,
            deployment: serde_json::Value,
        }

        let raw = Raw::deserialize(deserializer)?;

        let deployment = match raw.metadata.environment {
            Environment::Linux | Environment::Windows => {
                let on_host: OnHost =
                    serde_json::from_value(raw.deployment).map_err(D::Error::custom)?;
                Deployment::Host(on_host)
            }
            Environment::K8s => {
                let k8s: K8s = serde_json::from_value(raw.deployment).map_err(D::Error::custom)?;
                Deployment::K8s(k8s)
            }
        };

        Ok(AgentTypeDefinition {
            metadata: raw.metadata,
            variables: raw.variables,
            runtime_config: Runtime { deployment },
        })
    }
}

impl AgentTypeDefinition {
    pub fn agent_type_id(&self) -> &AgentTypeID {
        &self.metadata.id
    }

    /// Materializes this definition into an [AgentType] by applying the given variable
    /// constraints to the parsed variable tree.
    pub fn with_constraints(self, constraints: &VariableConstraints) -> AgentType {
        AgentType {
            agent_type_id: self.metadata.id,
            variables: self.variables.with_config(constraints),
            runtime_config: self.runtime_config,
        }
    }
}

#[derive(Error, Debug, PartialEq)]
pub enum AgentTypeMetadataError {
    #[error("operating_system is required when platform is host")]
    MissingOperatingSystem,
    #[error("operating_system must not be set when platform is kubernetes")]
    UnexpectedOperatingSystem,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Host,
    Kubernetes,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperatingSystem {
    Linux,
    Windows,
}

/// Holds the identity plus extra metadata that identifies a [AgentTypeDefinition]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTypeMetadata {
    pub id: AgentTypeID,
    pub environment: Environment,
}

impl AgentTypeMetadata {
    fn try_from_parts(
        id: AgentTypeID,
        platform: Platform,
        operating_system: Option<OperatingSystem>,
    ) -> Result<Self, AgentTypeMetadataError> {
        let environment = match (platform, operating_system) {
            (Platform::Host, Some(OperatingSystem::Linux)) => Environment::Linux,
            (Platform::Host, Some(OperatingSystem::Windows)) => Environment::Windows,
            (Platform::Kubernetes, None) => Environment::K8s,
            (Platform::Host, None) => {
                return Err(AgentTypeMetadataError::MissingOperatingSystem);
            }
            (Platform::Kubernetes, Some(_)) => {
                return Err(AgentTypeMetadataError::UnexpectedOperatingSystem);
            }
        };
        Ok(AgentTypeMetadata { id, environment })
    }
}

impl<'de> Deserialize<'de> for AgentTypeMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(flatten)]
            id: AgentTypeID,
            platform: Platform,
            operating_system: Option<OperatingSystem>,
        }

        let Raw {
            id,
            platform,
            operating_system,
        } = Raw::deserialize(deserializer)?;

        AgentTypeMetadata::try_from_parts(id, platform, operating_system).map_err(D::Error::custom)
    }
}

/// The agent type after constraints have been applied to its variables. This is the form the
/// renderer consumes: `variables` is a [VariableTree] ready to be filled with values.
#[derive(Debug, PartialEq, Clone)]
pub struct AgentType {
    pub agent_type_id: AgentTypeID,
    pub variables: VariableTree,
    pub runtime_config: Runtime,
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
    values: HashMap<String, serde_json::Value>,
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
                let v: HashMap<String, serde_json::Value> = serde_json::from_value(value)?;
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

pub fn include_packages_variables(
    mut variables: Variables,
    packages: &RenderedPackages,
) -> Result<Variables, AgentTypeError> {
    // Return early if no packages to avoid retrieving the filesystem dir unnecessarily
    if packages.is_empty() {
        return Ok(variables);
    }

    let remote_dir = &get_sub_agent_variable(&variables, AgentAttributes::VARIABLE_REMOTE_DIR)
        .ok_or(AgentTypeError::RenderingTemplate(format!(
            "Agent variable not found {}",
            AgentAttributes::VARIABLE_REMOTE_DIR
        )))?;

    let agent_id_string =
        get_sub_agent_variable(&variables, AgentAttributes::VARIABLE_SUB_AGENT_ID).ok_or(
            AgentTypeError::RenderingTemplate(format!(
                "Agent variable not found {}",
                AgentAttributes::VARIABLE_SUB_AGENT_ID
            )),
        )?;

    let agent_id = AgentID::try_from(agent_id_string)
        .map_err(|e| AgentTypeError::RenderingTemplate(format!("Invalid sub-agent ID: {}", e)))?;

    for (package_id, package) in packages {
        let package_data = PackageData {
            id: package_id.to_string(),
            oci: package.download.oci.clone(),
        };
        let path =
            get_package_path(Path::new(remote_dir), &agent_id, &package_data).map_err(|e| {
                AgentTypeError::RenderingTemplate(format!(
                    "Invalid OCI reference for package {}: {}",
                    package_id, e
                ))
            })?;
        debug!(package_id = %package_id, path = %path.display(), "Setting reserved variable for package directory");

        variables.insert(
            Namespace::SubAgent.namespaced_name(format!("packages.{}.dir", package_id)),
            Variable::new_final_string_variable(path.to_string_lossy()),
        );
    }

    Ok(variables)
}

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
    use crate::agent_type::trivial_value::TrivialValue;
    use crate::agent_type::variable::constraints::VariableConstraints;
    use rstest::rstest;
    use serde_json::Number;
    use serde_saphyr::Error;
    use std::collections::HashMap as Map;

    impl AgentTypeDefinition {
        /// This helper returns an [AgentTypeDefinition] including only the provided id.
        /// Defaults to a k8s deployment with empty variables.
        pub fn empty_with_metadata(id: AgentTypeID) -> Self {
            Self {
                metadata: AgentTypeMetadata {
                    id,
                    environment: Environment::K8s,
                },
                variables: VariableDefinitionTree::default(),
                runtime_config: Runtime {
                    deployment: Deployment::K8s(K8s::default()),
                },
            }
        }
    }

    impl AgentType {
        /// Builds a testing [AgentType] from the given YAML by deserializing it as an
        /// [AgentTypeDefinition] and applying default variable constraints.
        ///
        /// # Panics
        ///
        /// The function will panic if the definition is not valid.
        pub fn build_for_testing(yaml_definition: &str) -> Self {
            let definition =
                serde_saphyr::from_str::<AgentTypeDefinition>(yaml_definition).unwrap();
            definition.with_constraints(&VariableConstraints::default())
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
            let values = serde_saphyr::from_str::<YAMLConfig>(yaml_values).unwrap();
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
platform: host
operating_system: linux
variables:
  description:
    name:
      description: "Name of the agent"
      type: string
      required: false
      default: nrdot
deployment:
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
      args:
        - -c
        - ${nr-var:deployment.k8s.image}
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
platform: host
operating_system: linux
variables:
  description:
    name:
"#;

    #[test]
    fn test_basic_agent_parsing() {
        let basic_agent = r#"
name: nrdot
namespace: newrelic
version: 0.0.1
platform: host
operating_system: linux
variables: {}
deployment: {}
"#;

        let agent: AgentTypeDefinition = serde_saphyr::from_str(basic_agent).unwrap();

        assert_eq!("nrdot", agent.agent_type_id().name());
        assert_eq!("newrelic", agent.agent_type_id().namespace());
        assert_eq!("0.0.1", agent.agent_type_id().version().to_string());
    }

    #[test]
    fn test_bad_parsing() {
        let raw_agent_err: Result<AgentTypeDefinition, Error> =
            serde_saphyr::from_str(AGENT_GIVEN_BAD_YAML);

        assert!(raw_agent_err.is_err());
        assert!(
            raw_agent_err
                .unwrap_err()
                .to_string()
                .contains("data did not match any variant of untagged enum Tree")
        );
    }

    #[test]
    fn test_missing_deployment_field_is_rejected() {
        let yaml = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
platform: kubernetes
variables: {}
"#;
        let err = serde_saphyr::from_str::<AgentTypeDefinition>(yaml).unwrap_err();
        assert!(
            err.to_string().contains("missing field `deployment`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_normalize_agent_spec() {
        // create AgentSpec
        let given_agent = AgentType::build_for_testing(AGENT_GIVEN_YAML);

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
platform: host
operating_system: linux
variables:
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
      args:
        - --config
        - ${nr-var:config}
        - --config2
        - ${nr-var:config2}
  packages:
    infra-agent:
      download:
        oci:
          registry: ${nr-var:registry}
          repository: ${nr-var:repository}
          public_key: ${nr-var:public_key}
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
        let input_agent_type = AgentType::build_for_testing(GIVEN_NEWRELIC_INFRA_YAML);
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
platform: host
operating_system: linux
variables:
  restart_policy:
    type:
      description: "restart policy type"
      type: string
      required: false
      variants:
        values: [fixed, linear]
      default: exponential
deployment:
  executables:
    - id: echo
      path: /bin/echo
      args:
        - "${nr-var:restart_policy.type}"
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
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_VARIANTS);

        // Valid variant
        let filled_variables = agent_type.fill_variables(VALUES_VALID_VARIANT);

        let var = filled_variables.get("restart_policy.type").unwrap();
        assert_eq!(
            "fixed".to_string(),
            var.get_final_value().unwrap().to_string()
        );

        // Invalid variant
        let invalid_values: YAMLConfig =
            serde_saphyr::from_str(VALUES_INVALID_VARIANT).expect("Failed to parse user config");
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

    #[rstest]
    #[case::host_linux("platform: host\noperating_system: linux", Ok(Environment::Linux))]
    #[case::host_windows("platform: host\noperating_system: windows", Ok(Environment::Windows))]
    #[case::kubernetes("platform: kubernetes", Ok(Environment::K8s))]
    #[case::host_without_os("platform: host", Err(AgentTypeMetadataError::MissingOperatingSystem))]
    #[case::kubernetes_with_os(
        "platform: kubernetes\noperating_system: linux",
        Err(AgentTypeMetadataError::UnexpectedOperatingSystem)
    )]
    fn test_agent_type_metadata_from_yaml(
        #[case] platform_block: &str,
        #[case] expected: Result<Environment, AgentTypeMetadataError>,
    ) {
        let yaml = format!(
            r#"
name: fake_name
namespace: fake_namespace
version: 0.0.1
{platform_block}
"#
        );

        let result = serde_saphyr::from_str::<AgentTypeMetadata>(&yaml);

        match expected {
            Ok(env) => assert_eq!(env, result.unwrap().environment),
            Err(err) => assert!(
                result.unwrap_err().to_string().contains(&err.to_string()),
                "expected error containing: {err}"
            ),
        }
    }
}
