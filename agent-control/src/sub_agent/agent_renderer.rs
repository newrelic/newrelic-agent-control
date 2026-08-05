//! Renders an [EffectiveAgent] (rendered runtime configuration) from an agent identity and
//! its YAML config, resolving the agent type, variables, and secrets.

use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::definition::{AgentType, VariableTree};
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::rendered::OnHost;
use crate::agent_type::runtime_config::rendered;
use crate::agent_type::templates::Templateable;
use crate::agent_type::variable::Variable;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::namespace::{Namespace, VariableName};
use crate::agent_type::variable::secret_variables::{SecretVariables, SecretVariablesError};
use crate::secrets_provider::SecretsProviders;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::{YAMLConfig, YAMLConfigError};
use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Errors produced while rendering an [EffectiveAgent].
#[derive(Error, Debug)]
pub enum AgentRendererError {
    /// A generic rendering failure with a descriptive message.
    #[error("{0}")]
    Generic(String),
    /// The agent type could not be retrieved from the registry.
    #[error("retrieving agent type: {0}")]
    Registry(#[from] AgentTypeRegistryError),
    /// YAML (de)serialization failed.
    #[error("deserializing yaml: {0}")]
    SerializationError(#[from] serde_saphyr::Error),
    /// A value could not be converted to/from JSON.
    #[error("converting value to json: {0}")]
    ValueConversionError(#[from] serde_json::Error),
    /// The agent type definition was invalid.
    #[error("rendering agent type: {0}")]
    AgentTypeError(#[from] AgentTypeError),
    /// Secret variables could not be loaded.
    #[error("loading secrets: {0}")]
    SecretVariablesError(#[from] SecretVariablesError),
}

/// An agent with its identity and fully rendered runtime configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveAgent {
    agent_identity: AgentIdentity,
    runtime_config: rendered::Runtime,
}

impl Display for EffectiveAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.agent_identity.id.to_string())
    }
}

impl EffectiveAgent {
    pub(crate) fn new(agent_identity: AgentIdentity, runtime_config: rendered::Runtime) -> Self {
        Self {
            agent_identity,
            runtime_config,
        }
    }

    pub(crate) fn get_onhost_config(&self) -> Result<&OnHost, AgentRendererError> {
        match &self.runtime_config.deployment {
            rendered::Deployment::Host(on_host) => Ok(on_host),
            rendered::Deployment::K8s(_) => Err(AgentRendererError::Generic(
                "missing host deployment configuration".to_string(),
            )),
        }
    }

    pub(crate) fn get_k8s_config(&self) -> Result<&K8s, AgentRendererError> {
        match &self.runtime_config.deployment {
            rendered::Deployment::K8s(k8s) => Ok(k8s),
            rendered::Deployment::Host(_) => Err(AgentRendererError::Generic(
                "missing k8s deployment configuration".to_string(),
            )),
        }
    }

    pub(crate) fn get_agent_identity(&self) -> &AgentIdentity {
        &self.agent_identity
    }
}

impl TryFrom<EffectiveAgent> for K8s {
    type Error = AgentRendererError;

    fn try_from(value: EffectiveAgent) -> Result<Self, Self::Error> {
        match value.runtime_config.deployment {
            rendered::Deployment::K8s(k8s) => Ok(k8s),
            rendered::Deployment::Host(_) => Err(AgentRendererError::Generic(
                "missing k8s deployment configuration".to_string(),
            )),
        }
    }
}

/// Renders an [EffectiveAgent] from an agent identity and its YAML configuration.
pub trait AgentRenderer {
    /// Renders an [EffectiveAgent] from an [AgentIdentity]. The implementer is responsible for
    /// getting the AgentType and all needed values to render the Runtime config.
    fn render_agent(
        &self,
        agent_identity: &AgentIdentity,
        yaml_config: YAMLConfig,
    ) -> Result<EffectiveAgent, AgentRendererError>;
}

/// Implements [AgentRenderer] and is responsible for:
/// - Getting [`AgentType`](crate::agent_type::definition::AgentType) from [AgentTypeRegistry]
/// - Getting Local or Remote configs from [`ConfigRepository`](crate::values::config_repository::ConfigRepository)
/// - Rendering the [`Runtime`](crate::agent_type::runtime_config::Runtime) configuration of an Agent
///
/// Important: Rendering an Agent may mutate the state of external resources by creating
/// or removing configs when the `Runtime` is rendered.
pub struct DefaultAgentRenderer<R>
where
    R: AgentTypeRegistry,
{
    registry: Arc<R>,
    ac_variables: HashMap<VariableName, Variable>,
    variable_constraints: VariableConstraints,
    secrets_providers: SecretsProviders,
    remote_dir: PathBuf,
}

impl<R> DefaultAgentRenderer<R>
where
    R: AgentTypeRegistry,
{
    /// Creates a renderer from an agent-type registry, agent-control variables, variable
    /// constraints, secrets providers, and the remote configuration directory.
    pub fn new(
        registry: Arc<R>,
        ac_variables: impl Iterator<Item = (String, Variable)>,
        variable_constraints: VariableConstraints,
        secrets_providers: SecretsProviders,
        remote_dir: &Path,
    ) -> Self {
        DefaultAgentRenderer {
            registry,
            ac_variables: namespace_agent_control_variables(ac_variables),
            variable_constraints,
            secrets_providers,
            remote_dir: remote_dir.to_path_buf(),
        }
    }
}

impl<R> AgentRenderer for DefaultAgentRenderer<R>
where
    R: AgentTypeRegistry,
{
    fn render_agent(
        &self,
        agent_identity: &AgentIdentity,
        values: YAMLConfig,
    ) -> Result<EffectiveAgent, AgentRendererError> {
        // Load the parsed definition and apply the AC-wide variable constraints to materialize
        // an [AgentType] ready for the renderer.
        let agent_type = self
            .registry
            .get(&agent_identity.agent_type_id)?
            .with_constraints(&self.variable_constraints);

        let agent_variables = AgentAttributes::get_agent_variables(
            agent_identity.id.to_owned(),
            self.remote_dir.to_path_buf(),
        )
        .map_err(|e| AgentRendererError::Generic(e.to_string()))?;

        let user_values: String = values
            .clone()
            .try_into()
            .map_err(|e: YAMLConfigError| SecretVariablesError::YamlParseError(e.to_string()))?;
        let runtime: String = serde_json::to_string(&agent_type.runtime_config)
            .map_err(|e| SecretVariablesError::YamlParseError(e.to_string()))?;

        let secret_variables_values = SecretVariables::from(user_values.as_str());
        let secret_variables_runtime = SecretVariables::from(runtime.as_str());

        let mut secrets: HashMap<VariableName, Variable> = HashMap::new();
        secrets.extend(secret_variables_values.load_secrets(&self.secrets_providers)?);
        secrets.extend(secret_variables_runtime.load_secrets(&self.secrets_providers)?);

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            agent_variables,
            secrets,
            self.ac_variables.clone(),
        )?;

        Ok(EffectiveAgent::new(agent_identity.clone(), runtime_config))
    }
}

/// Renders an [`AgentType`] together with user values into a runtime configuration for a sub-agent.
pub(crate) fn render_runtime_config(
    agent_type: AgentType,
    values: YAMLConfig,
    agent_variables: HashMap<VariableName, Variable>,
    secrets: HashMap<VariableName, Variable>,
    ac_variables: HashMap<VariableName, Variable>,
) -> Result<rendered::Runtime, AgentTypeError> {
    // Get empty variables and runtime_config from the agent-type
    let (variable_tree, runtime_config) = (agent_type.variables, agent_type.runtime_config);

    let expanded_user_variables = get_expanded_user_variables(variable_tree, values, &secrets)?;

    // Join all variables together, namespaced
    let ns_variables = expanded_user_variables
        .into_iter()
        .chain(agent_variables)
        .chain(secrets)
        .chain(ac_variables)
        .collect::<HashMap<VariableName, Variable>>();

    // Render runtime config
    let rendered_runtime_config = runtime_config.template_with(&ns_variables)?;

    Ok(rendered_runtime_config)
}

fn get_expanded_user_variables(
    variable_tree: VariableTree,
    values: YAMLConfig,
    secrets: &HashMap<VariableName, Variable>,
) -> Result<HashMap<VariableName, Variable>, AgentTypeError> {
    // Values are expanded substituting all ${nr-env, nr-values} performing double expansions.
    // Notice that only data coming from secrets providers taken into consideration (no other vars for example)
    let values_expanded = values.template_with(secrets)?;

    // Fill agent data in the variables tree
    let flat_variable_tree = variable_tree.fill_with_values(values_expanded)?.flatten();
    check_all_vars_are_populated(&flat_variable_tree)?;
    // Set the namespaced name to variables
    Ok(flat_variable_tree
        .into_iter()
        .map(|(name, var)| (VariableName::new(Namespace::Variable, &name), var))
        .collect())
}

fn check_all_vars_are_populated(
    variables: &HashMap<String, Variable>,
) -> Result<(), AgentTypeError> {
    let not_populated = variables
        .clone()
        .into_iter()
        .filter_map(|(k, endspec)| endspec.get_final_value().is_none().then_some(k))
        .collect::<Vec<_>>();
    if !not_populated.is_empty() {
        return Err(AgentTypeError::ValuesNotPopulated(not_populated));
    }
    Ok(())
}

/// Namespaces a set of agent-control variables (identified by name) under the
/// [`Namespace::AgentControl`] namespace.
pub(crate) fn namespace_agent_control_variables(
    variables: impl Iterator<Item = (String, Variable)>,
) -> HashMap<VariableName, Variable> {
    variables
        .map(|(name, value)| {
            (
                VariableName::new(Namespace::AgentControl, name.as_str()),
                value,
            )
        })
        .collect()
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[allow(missing_docs)]
pub(crate) mod tests {

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use crate::agent_type::runtime_config::on_host::executable::rendered as exec_rendered;
    use crate::agent_type::runtime_config::restart_policy::{
        BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
    };
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use mockall::mock;
    use std::str::FromStr;

    mock! {
        pub AgentRenderer {}

        impl AgentRenderer for AgentRenderer {
            fn render_agent(
                &self,
                agent_identity:&AgentIdentity,
                yaml_config: YAMLConfig,
            ) -> Result<EffectiveAgent, AgentRendererError>;

        }
    }

    impl<R> DefaultAgentRenderer<R>
    where
        R: AgentTypeRegistry,
    {
        pub fn new_for_testing(registry: R) -> Self {
            Self {
                registry: Arc::new(registry),
                ac_variables: HashMap::new(),
                variable_constraints: VariableConstraints::default(),
                secrets_providers: SecretsProviders::default(),
                remote_dir: PathBuf::default(),
            }
        }
    }

    #[test]
    fn test_render_agent() {
        // Mocks
        let mut registry = MockAgentTypeRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
        ));
        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());
        let values = YAMLConfig::default();

        //Expectations
        registry.should_get(
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
            &agent_type_definition,
        );

        let renderer = DefaultAgentRenderer::new_for_testing(registry);

        let effective_agent = renderer.render_agent(&agent_identity, values).unwrap();

        assert_eq!(agent_identity, effective_agent.agent_identity);
    }

    #[test]
    fn test_render_agent_error_on_registry() {
        //Mocks
        let mut registry = MockAgentTypeRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
        ));

        //Expectations
        registry.expect_get_not_found(AgentTypeID::try_from("namespace/name:0.0.1").unwrap());
        let renderer = DefaultAgentRenderer::new_for_testing(registry);

        let result = renderer.render_agent(&agent_identity, YAMLConfig::default());

        assert!(result.is_err());
        assert_eq!(
            "retrieving agent type: agent type namespace/name:0.0.1 not found",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_with_constraints_k8s() {
        let definition =
            serde_saphyr::from_str::<AgentTypeDefinition>(K8S_AGENT_TYPE_DEFINITION).unwrap();

        let agent_type = definition.with_constraints(&VariableConstraints::default());

        let vars = agent_type.variables.flatten();
        assert_eq!(
            "K8s var".to_string(),
            vars.get("config.var").unwrap().description
        );
        assert!(matches!(
            agent_type.runtime_config.deployment,
            crate::agent_type::runtime_config::Deployment::K8s(_)
        ));
    }

    #[test]
    fn test_with_constraints_host_linux() {
        let definition =
            serde_saphyr::from_str::<AgentTypeDefinition>(HOST_LINUX_AGENT_TYPE_DEFINITION)
                .unwrap();

        let agent_type = definition.with_constraints(&VariableConstraints::default());

        let vars = agent_type.variables.flatten();
        assert_eq!(
            "Linux var".to_string(),
            vars.get("config.var").unwrap().description
        );
        assert!(matches!(
            agent_type.runtime_config.deployment,
            crate::agent_type::runtime_config::Deployment::Host(_)
        ));
    }

    const K8S_AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
platform: kubernetes
variables:
  config:
    var:
      description: "K8s var"
      type: string
      required: true
deployment:
  objects:
    chart:
      apiVersion: some.api.version/v1
      kind: SomeKind
      metadata:
        name: ${nr-sub:agent_id}
        namespace: ${nr-ac:namespace}
      spec:
        other: ${nr-var:config.var}
"#;

    const HOST_LINUX_AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
platform: host
operating_system: linux
variables:
  config:
    var:
      description: "Linux var"
      type: string
      required: true
deployment:
  executables:
    - id: my-exec
      path: /some/path
      args:
        - ${nr-var:config.var}
"#;

    pub fn testing_agent_attributes(agent_id: &AgentID) -> HashMap<VariableName, Variable> {
        #[cfg(windows)]
        let root = "C:\\";
        #[cfg(not(windows))]
        let root = "/";

        AgentAttributes::get_agent_variables(agent_id.clone(), PathBuf::from_str(root).unwrap())
            .unwrap()
    }

    fn testing_values(yaml_values: &str) -> YAMLConfig {
        serde_saphyr::from_str(yaml_values).unwrap()
    }

    #[test]
    fn test_render() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE);
        let values = testing_values(SIMPLE_AGENT_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        )
        .unwrap();

        let mut bin_stack = vec!["/opt/first", "/opt/second"].into_iter();
        runtime_config
            .deployment
            .on_host()
            .executables
            .iter()
            .for_each(|exec| {
                assert_eq!(bin_stack.next().unwrap(), exec.path.clone());
                assert_eq!(
                    exec_rendered::Args(vec!(
                        "--config_path".to_string(),
                        "/some/path/config".to_string(),
                        "--foo".to_string(),
                        "bar".to_string()
                    )),
                    exec.args.clone()
                );
            });
    }

    #[test]
    fn test_render_with_empty_but_required_values() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE);
        let values = YAMLConfig::default();
        let attributes = testing_agent_attributes(&agent_id);

        let result = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );
        assert_matches!(result.unwrap_err(), AgentTypeError::ValuesNotPopulated(vars) => {
            assert_eq!(vars, vec!["config_path".to_string()])
        })
    }

    #[test]
    fn test_render_with_missing_values() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(SIMPLE_AGENT_TYPE);
        let values = testing_values(SIMPLE_AGENT_VALUES_REQUIRED_MISSING);
        let attributes = testing_agent_attributes(&agent_id);

        let result = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );
        assert_matches!(result.unwrap_err(), AgentTypeError::ValuesNotPopulated(vars) => {
            assert_eq!(vars, vec!["config_path".to_string()])
        })
    }

    #[test]
    fn test_render_agent_type_with_backoff_config() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF);
        let values = testing_values(BACKOFF_VALUES_YAML);
        let attributes = testing_agent_attributes(&agent_id);

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        )
        .unwrap();

        let on_host_deployment = runtime_config.deployment.on_host();

        let backoff_strategy = &on_host_deployment
            .executables
            .first()
            .unwrap()
            .restart_policy
            .backoff_strategy;
        assert_eq!(
            BackoffStrategyType::Linear,
            backoff_strategy.backoff_type.clone()
        );
        assert_eq!(
            BackoffDelay::from_secs(10),
            backoff_strategy.backoff_delay.clone()
        );
        assert_eq!(MaxRetries::from(30), backoff_strategy.max_retries.clone());
        assert_eq!(
            BackoffLastRetryInterval::from_secs(300),
            backoff_strategy.last_retry_interval.clone()
        );
    }

    #[test]
    fn test_render_agent_type_with_backoff_config_and_string_durations() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF);
        let values = testing_values(BACKOFF_VALUES_STRING_DURATION);
        let attributes = testing_agent_attributes(&agent_id);

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        )
        .unwrap();

        let on_host_deployment = runtime_config.deployment.on_host();
        let backoff_strategy = &on_host_deployment
            .executables
            .first()
            .unwrap()
            .restart_policy
            .backoff_strategy;
        assert_eq!(
            BackoffStrategyType::Fixed,
            backoff_strategy.backoff_type.clone()
        );
        assert_eq!(
            BackoffDelay::from_secs((10 * 60) + 30),
            backoff_strategy.backoff_delay.clone()
        );
        assert_eq!(MaxRetries::from(30), backoff_strategy.max_retries.clone());
        assert_eq!(
            BackoffLastRetryInterval::from_secs(300),
            backoff_strategy.last_retry_interval.clone()
        );
    }

    #[test]
    fn test_invalid_values_for_backoff_config() {
        // This is testing agent-type definition and values, but it is included here because it its related to
        // test_render_agent_type_with_backoff_config.
        let agent_type = AgentType::build_for_testing(AGENT_TYPE_WITH_BACKOFF);

        let wrong_backoff_yamls = vec![
            WRONG_RETRIES_BACKOFF_CONFIG_YAML,
            WRONG_DELAY_BACKOFF_CONFIG_YAML,
            WRONG_INTERVAL_BACKOFF_CONFIG_YAML,
            WRONG_TYPE_BACKOFF_CONFIG_YAML,
        ];

        for yaml in wrong_backoff_yamls.into_iter() {
            let values = serde_saphyr::from_str::<YAMLConfig>(yaml).unwrap();
            assert!(
                agent_type
                    .variables
                    .clone()
                    .fill_with_values(values)
                    .is_err()
            )
        }
    }

    #[test]
    fn test_render_k8s_config_with_yaml_variables() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let expected_spec_yaml = r#"
values:
  another_key:
    nested: nested_value ${UNTOUCHED}
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
from_sub_agent: some-agent-id
text_values: "key: value\nkey2: ${UNTOUCHED}\n\n"
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;
        let expected_spec_value: serde_json::Value =
            serde_saphyr::from_str(expected_spec_yaml).unwrap();

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        )
        .unwrap();

        let k8s = runtime_config.deployment.k8s();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    fn test_render_with_env_variables() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let env_vars = HashMap::from([
            (
                VariableName::new(Namespace::EnvironmentVariable, "MY_VARIABLE"),
                Variable::new_final_string_variable("my-value".to_string()),
            ),
            (
                VariableName::new(Namespace::EnvironmentVariable, "MY_VARIABLE_2"),
                Variable::new_final_string_variable("my-value-2".to_string()),
            ),
        ]);

        let expected_spec_yaml = r#"
values:
  another_key:
    nested: nested_value ${UNTOUCHED}
    nested_list:
      - item1
      - item2
      - item3_nested: value
  empty_key:
from_sub_agent: some-agent-id
substituted: my-value
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
substituted_2: my-value-2
"#;

        let expected_spec_value: serde_json::Value =
            serde_saphyr::from_str(expected_spec_yaml).unwrap();

        let runtime_config =
            render_runtime_config(agent_type, values, attributes, env_vars, HashMap::new());

        let k8s = runtime_config.unwrap().deployment.k8s();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    fn test_render_double_expansion_with_env_variables() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES);
        let values = testing_values(
            r#"
config:
  text_values:
    key: value
    key2: ${UNTOUCHED}
  values:
    key: ${nr-env:DOUBLE_EXPANSION}
    key-2: ${nr-env:DOUBLE_EXPANSION_2}
"#,
        );
        let attributes = testing_agent_attributes(&agent_id);

        let secrets = HashMap::from([
            (
                VariableName::new(Namespace::EnvironmentVariable, "DOUBLE_EXPANSION"),
                Variable::new_final_string_variable("test".to_string()),
            ),
            (
                VariableName::new(Namespace::EnvironmentVariable, "DOUBLE_EXPANSION_2"),
                Variable::new_final_string_variable("test-2".to_string()),
            ),
        ]);

        let expected_spec_yaml = r#"
values:
  key: test
  key-2: test-2
from_sub_agent: some-agent-id
text_values: "key: value\nkey2: ${UNTOUCHED}\n\n"
collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;

        let expected_spec_value: serde_json::Value =
            serde_saphyr::from_str(expected_spec_yaml).unwrap();

        let runtime_config =
            render_runtime_config(agent_type, values, attributes, secrets, HashMap::new());

        let k8s = runtime_config.unwrap().deployment.k8s();
        let values = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();
        assert_eq!(expected_spec_value, values.clone());
    }

    #[test]
    fn test_render_with_env_variables_not_found() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let runtime_config = render_runtime_config(
            agent_type,
            values,
            attributes,
            HashMap::new(),
            HashMap::new(),
        );

        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }

    #[test]
    fn test_render_with_env_variables_are_case_sensitive() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let agent_type = AgentType::build_for_testing(
            r#"
name: k8s_agent_type
namespace: newrelic
version: 0.0.1
platform: kubernetes
variables:
  config:
    values:
      description: "yaml values"
      type: yaml
      required: true
    text_values:
      description: "yaml values"
      type: yaml
      required: true
deployment:
  objects:
    cr1:
      apiVersion: group/version
      kind: ObjectKind
      metadata:
        name: test
        namespace: test-namespace
      substituted: ${nr-env:MY_VARIABLE}
"#,
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);
        let attributes = testing_agent_attributes(&agent_id);

        let env_vars = HashMap::from([(
            VariableName::new(Namespace::EnvironmentVariable, "my_variable"),
            Variable::new_final_string_variable("my-value".to_string()),
        )]);

        let runtime_config =
            render_runtime_config(agent_type, values, attributes, env_vars, HashMap::new());

        assert_matches!(
            runtime_config.unwrap_err(),
            AgentTypeError::MissingTemplateKey(_)
        );
    }

    #[test]
    fn test_render_expand_agent_control_variables() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();

        let agent_type = AgentType::build_for_testing(
            r#"
namespace: newrelic
name: first
version: 0.1.0
platform: host
operating_system: linux
variables: {}
deployment:
  executables:
    - id: first
      path: /opt/first
      args:
        - "${nr-ac:sa-fake-var}"
"#,
        );
        let values = testing_values("");
        let attributes = testing_agent_attributes(&agent_id);

        let agent_control_variables = HashMap::from([(
            "sa-fake-var".to_string(),
            Variable::new_final_string_variable("fake_value".to_string()),
        )]);
        let ac_variables = namespace_agent_control_variables(agent_control_variables.into_iter());

        let runtime_config =
            render_runtime_config(agent_type, values, attributes, HashMap::new(), ac_variables)
                .unwrap();
        assert_eq!(
            exec_rendered::Args(vec!("fake_value".to_string())),
            runtime_config
                .deployment
                .on_host()
                .executables
                .first()
                .unwrap()
                .args
                .clone()
        );
    }

    #[test]
    fn test_render_env_in_runtime_and_all_secrets_expand_user_values() {
        let agent_id = AgentID::try_from("some-agent-id").unwrap();
        let attributes = testing_agent_attributes(&agent_id);

        let agent_type = AgentType::build_for_testing(
            r#"
name: k8s_agent_type
namespace: newrelic
version: 0.0.1
platform: kubernetes
variables:
  my_yaml:
    description: "yaml with double-expanded secrets"
    type: yaml
    required: true
deployment:
  objects:
    cr1:
      apiVersion: group/version
      kind: ObjectKind
      metadata:
        name: test
        namespace: test-namespace
      spec:
        vault_field: ${nr-vault:V_KEY}
        env_direct: ${nr-env:ENV}
        from_sub_agent: ${nr-sub:agent_id}
        values: ${nr-var:my_yaml}
"#,
        );

        let values = testing_values(
            r#"
my_yaml:
  vault_field: ${nr-vault:V_KEY}
  kubesec_field: ${nr-kubesec:K_KEY}
  env_field: ${nr-env:ENV}
"#,
        );

        let secrets = HashMap::from([
            (
                VariableName::new(Namespace::EnvironmentVariable, "ENV"),
                Variable::new_final_string_variable("env-value".to_string()),
            ),
            (
                VariableName::new(Namespace::Vault, "V_KEY"),
                Variable::new_final_string_variable("vault-value".to_string()),
            ),
            (
                VariableName::new(Namespace::K8sSecret, "K_KEY"),
                Variable::new_final_string_variable("kubesec-value".to_string()),
            ),
        ]);

        let expected_spec_yaml = r#"
vault_field: vault-value
env_direct: env-value
from_sub_agent: some-agent-id
values:
  vault_field: vault-value
  kubesec_field: kubesec-value
  env_field: env-value
"#;
        let expected_spec: serde_json::Value = serde_saphyr::from_str(expected_spec_yaml).unwrap();

        let rendered =
            render_runtime_config(agent_type, values, attributes, secrets, HashMap::new()).unwrap();
        let k8s = rendered.deployment.k8s();
        let spec = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();
        assert_eq!(&expected_spec, spec);
    }

    // Agent Type and Values definitions

    const SIMPLE_AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
platform: host
operating_system: linux
variables:
  config_path:
    description: "config file string"
    type: string
    required: true
  config_argument:
    description: "config argument"
    type: string
    required: false
    default: bar
deployment:
  executables:
    - id: first
      path: /opt/first
      args:
        - --config_path
        - ${nr-var:config_path}
        - --foo
        - ${nr-var:config_argument}
    - id: second
      path: /opt/second
      args:
      - --config_path
      - ${nr-var:config_path}
      - --foo
      - ${nr-var:config_argument}
"#;

    const SIMPLE_AGENT_VALUES: &str = r#"
config_path: /some/path/config
"#;

    const SIMPLE_AGENT_VALUES_REQUIRED_MISSING: &str = r#"
config_argument: value
"#;

    const AGENT_TYPE_WITH_BACKOFF: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
platform: host
operating_system: linux
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
  executables:
    - id: otelcol
      path: /just-an-example
      args:
      - -c
      - some-arg
      restart_policy:
        backoff_strategy:
          type: ${nr-var:backoff.type}
          backoff_delay: ${nr-var:backoff.delay}
          max_retries: ${nr-var:backoff.retries}
          last_retry_interval: ${nr-var:backoff.interval}
"#;

    const BACKOFF_VALUES_YAML: &str = r#"
backoff:
  delay: 10s
  retries: 30
  interval: 300s
  type: linear
"#;

    const BACKOFF_VALUES_STRING_DURATION: &str = r#"
backoff:
  delay: 10m + 30s
  retries: 30
  interval: 5m
  type: fixed
"#;

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

    const K8S_AGENT_TYPE_YAML_VARIABLES: &str = r#"
name: k8s_agent_type
namespace: newrelic
version: 0.0.1
platform: kubernetes
variables:
  config:
    values:
      description: "yaml values"
      type: yaml
      required: true
    text_values:
      description: "text values"
      type: yaml
      required: true
deployment:
  objects:
    cr1:
      apiVersion: group/version
      kind: ObjectKind
      metadata:
        name: test
        namespace: test-namespace
      spec:
        values: ${nr-var:config.values}
        from_sub_agent: ${nr-sub:agent_id}
        text_values: |
          ${nr-var:config.text_values}
        collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
"#;

    const K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES: &str = r#"
name: k8s_agent_type
namespace: newrelic
version: 0.0.1
platform: kubernetes
variables:
  config:
    values:
      description: "yaml values"
      type: yaml
      required: true
    text_values:
      description: "text values"
      type: yaml
      required: true
deployment:
  objects:
    cr1:
      apiVersion: group/version
      kind: ObjectKind
      metadata:
        name: test
        namespace: test-namespace
      spec:
        values: ${nr-var:config.values}
        from_sub_agent: ${nr-sub:agent_id}
        substituted: ${nr-env:MY_VARIABLE}
        collision_avoided: ${config.values}-${env:agent_id}-${UNTOUCHED}
        substituted_2: ${nr-env:MY_VARIABLE_2}
"#;

    const K8S_CONFIG_YAML_VALUES: &str = r#"
config:
  text_values:
    key: value
    key2: ${UNTOUCHED}
  values:
    another_key:
      nested: nested_value ${UNTOUCHED}
      nested_list:
        - item1
        - item2
        - item3_nested: value
    empty_key:"#;
}
