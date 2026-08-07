//! Renders an [RenderedAgent] (rendered runtime configuration) from an agent identity and
//! its YAML config, resolving the agent type, variables, and secrets.

use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::definition::VariableTree;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::rendered::OnHost;
use crate::agent_type::runtime_config::{Runtime, rendered};
use crate::agent_type::templates::Templateable;
use crate::agent_type::variable::Variable;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::namespace::{Namespace, VariableName};
use crate::agent_type::variable::secret_variables::{SecretVariables, SecretVariablesError};
use crate::secrets_provider::{Registry, SecretsProvider, SecretsProviderType};
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::{YAMLConfig, YAMLConfigError};
use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Errors produced while rendering an [RenderedAgent].
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
pub struct RenderedAgent {
    agent_identity: AgentIdentity,
    runtime_config: rendered::Runtime,
}

impl Display for RenderedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.agent_identity.id.to_string())
    }
}

impl RenderedAgent {
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

impl TryFrom<RenderedAgent> for K8s {
    type Error = AgentRendererError;

    fn try_from(value: RenderedAgent) -> Result<Self, Self::Error> {
        match value.runtime_config.deployment {
            rendered::Deployment::K8s(k8s) => Ok(k8s),
            rendered::Deployment::Host(_) => Err(AgentRendererError::Generic(
                "missing k8s deployment configuration".to_string(),
            )),
        }
    }
}

/// Renders an [RenderedAgent] from an agent identity and its YAML configuration.
pub trait Renderer {
    /// Renders an [RenderedAgent] from an [AgentIdentity]. The implementer is responsible for
    /// getting the AgentType and all needed values to render the Runtime config.
    fn render_agent(
        &self,
        agent_identity: &AgentIdentity,
        yaml_config: YAMLConfig,
    ) -> Result<RenderedAgent, AgentRendererError>;
}

/// Implements [Renderer] and is responsible for:
/// - Getting [`AgentType`](crate::agent_type::definition::AgentType) from [AgentTypeRegistry]
/// - Getting Local or Remote configs from [`ConfigRepository`](crate::values::config_repository::ConfigRepository)
/// - Rendering the [`Runtime`] configuration of an Agent
///
/// Important: Rendering an Agent may mutate the state of external resources by creating
/// or removing configs when the `Runtime` is rendered.
pub struct AgentRenderer<R, S = SecretsProviderType>
where
    R: AgentTypeRegistry,
    S: SecretsProvider,
{
    registry: Arc<R>,
    ac_variables: HashMap<VariableName, Variable>,
    variable_constraints: VariableConstraints,
    secrets_providers: Registry<S>,
    remote_dir: PathBuf,
}

impl<R, S> AgentRenderer<R, S>
where
    R: AgentTypeRegistry,
    S: SecretsProvider,
{
    /// Creates a renderer from an agent-type registry, agent-control variables, variable
    /// constraints, secrets providers, and the remote configuration directory.
    pub fn new(
        registry: Arc<R>,
        ac_variables: HashMap<VariableName, Variable>,
        variable_constraints: VariableConstraints,
        secrets_providers: Registry<S>,
        remote_dir: &Path,
    ) -> Self {
        AgentRenderer {
            registry,
            ac_variables,
            variable_constraints,
            secrets_providers,
            remote_dir: remote_dir.to_path_buf(),
        }
    }

    // Loads all secret variables referenced in the provided runtime and values.
    fn load_secrets(
        &self,
        runtime_config: &Runtime,
        values: YAMLConfig,
    ) -> Result<HashMap<VariableName, Variable>, AgentRendererError> {
        let user_values: String = values
            .clone()
            .try_into()
            .map_err(|e: YAMLConfigError| SecretVariablesError::YamlParseError(e.to_string()))?;
        let runtime: String = serde_json::to_string(runtime_config)
            .map_err(|e| SecretVariablesError::YamlParseError(e.to_string()))?;

        let secret_variables_values = SecretVariables::from(user_values.as_str());
        let secret_variables_runtime = SecretVariables::from(runtime.as_str());

        let mut secrets: HashMap<VariableName, Variable> = HashMap::new();
        secrets.extend(secret_variables_values.load_secrets(&self.secrets_providers)?);
        secrets.extend(secret_variables_runtime.load_secrets(&self.secrets_providers)?);

        Ok(secrets)
    }
}

impl<R, S> Renderer for AgentRenderer<R, S>
where
    R: AgentTypeRegistry,
    S: SecretsProvider,
{
    fn render_agent(
        &self,
        agent_identity: &AgentIdentity,
        values: YAMLConfig,
    ) -> Result<RenderedAgent, AgentRendererError> {
        // Load the parsed definition and apply the AC-wide variable constraints to materialize
        // an [AgentType] ready for the renderer.
        let agent_type = self
            .registry
            .get(&agent_identity.agent_type_id)?
            .with_constraints(&self.variable_constraints);

        let agent_attributes =
            AgentAttributes::try_new(agent_identity.id.to_owned(), self.remote_dir.to_path_buf())
                .map_err(|e| AgentRendererError::Generic(e.to_string()))?;

        let secrets = self.load_secrets(&agent_type.runtime_config, values.clone())?;

        let (variable_tree, runtime_config) = (agent_type.variables, agent_type.runtime_config);

        // Expand user values: raw values can themselves reference other variables (e.g.
        // ${nr-env:...}, ${nr-path:...}, ${nr-vault:...}), so resolve those before using
        // the values to fill the agent type's variable tree.
        let user_expansion_variables: HashMap<VariableName, Variable> = secrets
            .clone()
            .into_iter()
            .chain(agent_attributes.nr_path_variables())
            .collect();
        let expanded_user_variables =
            get_expanded_user_variables(variable_tree, values, &user_expansion_variables)?;

        // Join all available namespaced variables into a single lookup set of namespaced
        // variables used to template the runtime config.
        let ns_variables = expanded_user_variables
            .into_iter()
            .chain(agent_attributes.nr_sub_variables())
            .chain(secrets)
            .chain(self.ac_variables.clone())
            .collect::<HashMap<VariableName, Variable>>();

        let rendered_runtime_config = runtime_config.template_with(&ns_variables)?;

        Ok(RenderedAgent::new(
            agent_identity.clone(),
            rendered_runtime_config,
        ))
    }
}

fn get_expanded_user_variables(
    variable_tree: VariableTree,
    values: YAMLConfig,
    user_expansion_variables: &HashMap<VariableName, Variable>,
) -> Result<HashMap<VariableName, Variable>, AgentTypeError> {
    // Values are expanded substituting all ${nr-env, nr-values} performing double expansions.
    // Notice that only data coming from secrets providers taken into consideration (no other vars for example)
    let values_expanded = values.template_with(user_expansion_variables)?;

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

#[cfg(test)]
#[allow(missing_docs)]
pub(crate) mod tests {

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::definition::{AgentType, AgentTypeDefinition};
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use crate::agent_type::runtime_config::on_host::executable::rendered as exec_rendered;
    use crate::agent_type::runtime_config::restart_policy::{
        BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
    };
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use fs::directory_manager::DirectoryManagerFs;
    use fs::file::LocalFile;
    use mockall::mock;
    use tempfile::TempDir;

    mock! {
        pub AgentRenderer {}

        impl Renderer for AgentRenderer {
            fn render_agent(
                &self,
                agent_identity:&AgentIdentity,
                yaml_config: YAMLConfig,
            ) -> Result<RenderedAgent, AgentRendererError>;

        }
    }

    /// Error returned by [MockFixedSecretsProvider] when no fixed value is registered for a
    /// requested secret path.
    #[derive(Error, Debug)]
    #[error("no fixed secret registered for {0}")]
    pub struct FixedSecretError(String);

    mock! {
        pub FixedSecretsProvider {}

        impl SecretsProvider for FixedSecretsProvider {
            type Error = FixedSecretError;

            fn get_secret(&self, secret_path: &str) -> Result<String, FixedSecretError>;
        }
    }

    /// Builds a [`Registry`] that resolves `${nr-env:...}` references to the given fixed values,
    /// without registering the real [`Env`](crate::secrets_provider::env::Env) provider and
    /// therefore without touching real process environment variables.
    pub fn env_secrets_registry_for_testing(
        values: HashMap<&'static str, &'static str>,
    ) -> Registry<MockFixedSecretsProvider> {
        let values: HashMap<String, String> = values
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let mut provider = MockFixedSecretsProvider::new();
        provider.expect_get_secret().returning(move |path| {
            values
                .get(path)
                .cloned()
                .ok_or_else(|| FixedSecretError(path.to_string()))
        });

        Registry::from(HashMap::from([(Namespace::EnvironmentVariable, provider)]))
    }

    impl AgentRenderer<MockAgentTypeRegistry> {
        pub fn new_for_testing(registry: MockAgentTypeRegistry) -> Self {
            Self {
                registry: Arc::new(registry),
                ac_variables: HashMap::new(),
                variable_constraints: VariableConstraints::default(),
                secrets_providers: Registry::default(),
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

        let renderer = AgentRenderer::new_for_testing(registry);

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();

        assert_eq!(agent_identity, rendered_agent.agent_identity);
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
        let renderer = AgentRenderer::new_for_testing(registry);

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

    fn testing_values(yaml_values: &str) -> YAMLConfig {
        serde_saphyr::from_str(yaml_values).unwrap()
    }

    /// Builds a [MockAgentTypeRegistry] that resolves the given yaml definition and an
    /// [AgentIdentity] pointing at it, ready to be used with [AgentRenderer::render_agent].
    fn registry_for_testing(yaml_definition: &str) -> (MockAgentTypeRegistry, AgentIdentity) {
        let agent_type_definition =
            serde_saphyr::from_str::<AgentTypeDefinition>(yaml_definition).unwrap();
        let agent_type_id = agent_type_definition.metadata.id.clone();
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            agent_type_id.clone(),
        ));

        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(agent_type_id, &agent_type_definition);

        (registry, agent_identity)
    }

    #[test]
    fn test_render_agent_with_default_and_provided_values() {
        let (registry, agent_identity) = registry_for_testing(SIMPLE_AGENT_TYPE);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(SIMPLE_AGENT_VALUES);

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let on_host = rendered_agent.get_onhost_config().unwrap();

        assert_eq!(
            exec_rendered::Args(vec![
                "--config_path".to_string(),
                "/some/path/config".to_string(),
                "--foo".to_string(),
                "bar".to_string(),
            ]),
            on_host.executables.first().unwrap().args.clone()
        );
    }

    #[test]
    fn test_render_agent_error_on_required_value_missing_from_empty_values() {
        let (registry, agent_identity) = registry_for_testing(SIMPLE_AGENT_TYPE);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = YAMLConfig::default();

        let result = renderer.render_agent(&agent_identity, values);

        assert_matches!(
            result.unwrap_err(),
            AgentRendererError::AgentTypeError(AgentTypeError::ValuesNotPopulated(vars)) => {
                assert_eq!(vars, vec!["config_path".to_string()])
            }
        )
    }

    #[test]
    fn test_render_agent_error_on_required_value_missing_from_provided_values() {
        let (registry, agent_identity) = registry_for_testing(SIMPLE_AGENT_TYPE);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(SIMPLE_AGENT_VALUES_REQUIRED_MISSING);

        let result = renderer.render_agent(&agent_identity, values);

        assert_matches!(
            result.unwrap_err(),
            AgentRendererError::AgentTypeError(AgentTypeError::ValuesNotPopulated(vars)) => {
                assert_eq!(vars, vec!["config_path".to_string()])
            }
        )
    }

    #[test]
    fn test_render_agent_type_with_backoff_config() {
        let (registry, agent_identity) = registry_for_testing(AGENT_TYPE_WITH_BACKOFF);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(BACKOFF_VALUES_YAML);

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let on_host_deployment = rendered_agent.get_onhost_config().unwrap();

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
        let (registry, agent_identity) = registry_for_testing(AGENT_TYPE_WITH_BACKOFF);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(BACKOFF_VALUES_STRING_DURATION);

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let on_host_deployment = rendered_agent.get_onhost_config().unwrap();
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
        let (registry, agent_identity) = registry_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);

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

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let k8s = rendered_agent.get_k8s_config().unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    fn test_render_with_env_variables() {
        let (registry, agent_identity) =
            registry_for_testing(K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES);
        let secrets_providers = env_secrets_registry_for_testing(HashMap::from([
            ("MY_VARIABLE", "my-value"),
            ("MY_VARIABLE_2", "my-value-2"),
        ]));
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            HashMap::new(),
            VariableConstraints::default(),
            secrets_providers,
            Path::new(""),
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);

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

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let k8s = rendered_agent.get_k8s_config().unwrap();
        let cr1 = k8s.objects.get("cr1").unwrap();

        assert_eq!("group/version".to_string(), cr1.api_version);
        assert_eq!("ObjectKind".to_string(), cr1.kind);

        let spec = cr1.fields.get("spec").unwrap().clone();
        assert_eq!(expected_spec_value, spec);
    }

    #[test]
    fn test_render_agent_double_expansion_with_env_variables() {
        let (registry, agent_identity) = registry_for_testing(K8S_AGENT_TYPE_YAML_VARIABLES);
        let secrets_providers = env_secrets_registry_for_testing(HashMap::from([
            ("DOUBLE_EXPANSION", "test"),
            ("DOUBLE_EXPANSION_2", "test-2"),
        ]));
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            HashMap::new(),
            VariableConstraints::default(),
            secrets_providers,
            Path::new(""),
        );
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

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let k8s = rendered_agent.get_k8s_config().unwrap();
        let spec = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();

        let expected_values: serde_json::Value = serde_saphyr::from_str(
            r#"
key: test
key-2: test-2
"#,
        )
        .unwrap();

        assert_eq!(expected_values, spec.get("values").cloned().unwrap());
    }

    #[test]
    fn test_render_with_env_variables_not_found() {
        let (registry, agent_identity) =
            registry_for_testing(K8S_AGENT_TYPE_YAML_ENVIRONMENT_VARIABLES);
        let renderer = AgentRenderer::new_for_testing(registry);
        let values = testing_values(K8S_CONFIG_YAML_VALUES);

        // No secrets provider is registered, so `${nr-env:MY_VARIABLE}` and
        // `${nr-env:MY_VARIABLE_2}` in the runtime config are never resolved.
        let result = renderer.render_agent(&agent_identity, values);

        assert_matches!(
            result.unwrap_err(),
            AgentRendererError::AgentTypeError(AgentTypeError::MissingTemplateKey(_))
        );
    }

    #[test]
    fn test_render_with_env_variables_are_case_sensitive() {
        let (registry, agent_identity) = registry_for_testing(
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
        // The template references `MY_VARIABLE` (uppercase); the provider only knows the
        // lowercase path, so secret loading itself fails before templating is ever reached.
        let secrets_providers =
            env_secrets_registry_for_testing(HashMap::from([("my_variable", "my-value")]));
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            HashMap::new(),
            VariableConstraints::default(),
            secrets_providers,
            Path::new(""),
        );
        let values = testing_values(K8S_CONFIG_YAML_VALUES);

        let result = renderer.render_agent(&agent_identity, values);

        assert_matches!(
            result.unwrap_err(),
            AgentRendererError::SecretVariablesError(_)
        );
    }

    #[test]
    fn test_render_expand_agent_control_variables() {
        let (registry, agent_identity) = registry_for_testing(
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

        let agent_control_variables = HashMap::from([(
            VariableName::new(Namespace::AgentControl, "sa-fake-var"),
            Variable::new_final_string_variable("fake_value".to_string()),
        )]);
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            agent_control_variables,
            VariableConstraints::default(),
            Registry::<SecretsProviderType>::default(),
            Path::new(""),
        );
        let values = testing_values("");

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let on_host = rendered_agent.get_onhost_config().unwrap();

        assert_eq!(
            exec_rendered::Args(vec!("fake_value".to_string())),
            on_host.executables.first().unwrap().args.clone()
        );
    }

    #[test]
    fn test_render_env_in_runtime_and_all_secrets_expand_user_values() {
        let (registry, agent_identity) = registry_for_testing(
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

        let mut vault = MockFixedSecretsProvider::new();
        vault
            .expect_get_secret()
            .returning(|_| Ok("vault-value".to_string()));
        let mut env = MockFixedSecretsProvider::new();
        env.expect_get_secret()
            .returning(|_| Ok("env-value".to_string()));
        let mut kubesec = MockFixedSecretsProvider::new();
        kubesec
            .expect_get_secret()
            .returning(|_| Ok("kubesec-value".to_string()));

        let secrets_providers = Registry::from(HashMap::from([
            (Namespace::Vault, vault),
            (Namespace::EnvironmentVariable, env),
            (Namespace::K8sSecret, kubesec),
        ]));
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            HashMap::new(),
            VariableConstraints::default(),
            secrets_providers,
            Path::new(""),
        );

        let values = testing_values(
            r#"
my_yaml:
  vault_field: ${nr-vault:V_KEY}
  kubesec_field: ${nr-kubesec:K_KEY}
  env_field: ${nr-env:ENV}
"#,
        );

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

        let rendered_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let k8s = rendered_agent.get_k8s_config().unwrap();
        let spec = k8s.objects.get("cr1").unwrap().fields.get("spec").unwrap();
        assert_eq!(&expected_spec, spec);
    }

    #[test]
    fn test_render_expand_user_values_with_nr_path_agent_dir() {
        let (registry, agent_identity) = registry_for_testing(
            r#"
name: first
namespace: newrelic
version: 0.1.0
platform: host
operating_system: linux
variables:
  config:
    description: ""
    type: string
    required: true
  extra_configs:
    description: ""
    type: string_map
    required: true
deployment:
  filesystem:
    extra_configs:
      kind: dir_content_from_map
      source: ${nr-var:extra_configs}
    config.toml:
      kind: file
      text: ${nr-var:config}
  executables:
    - id: first
      path: /opt/first
      args:
        - --config
        - ${nr-sub:filesystem_agent_dir}/config.toml
"#,
        );

        // A real temp dir gives an absolute, platform-native remote dir, needed so the
        // filesystem entries can actually be written and read back below.
        let tmp_dir = TempDir::new().unwrap();
        let remote_dir = tmp_dir.path().to_path_buf();
        let renderer = AgentRenderer::new(
            Arc::new(registry),
            HashMap::new(),
            VariableConstraints::default(),
            Registry::<SecretsProviderType>::default(),
            &remote_dir,
        );

        let values = testing_values(
            r#"
config: |
  extra = ${nr-path:agent_dir}/extra_configs/extra.txt

extra_configs:
  extra.txt: |
    SOME CONTENT
"#,
        );

        let effective_agent = renderer.render_agent(&agent_identity, values).unwrap();
        let on_host = effective_agent.get_onhost_config().unwrap();

        // `nr-path:agent_dir` is resolved while expanding user values, so it must show up
        // already substituted in the written file contents.
        on_host
            .filesystem
            .write(&LocalFile, &DirectoryManagerFs)
            .unwrap();

        // Only `${nr-path:agent_dir}` is substituted with a native path; the rest of the
        // template (`/extra_configs/extra.txt`) is literal text, not a path join, so it keeps
        // its forward slashes on every platform. This is not an issue because there are specific
        // agent types for each platform.
        let expected_agent_dir = remote_dir.join("filesystem").join("some-agent-id");
        let config_content =
            std::fs::read_to_string(expected_agent_dir.join("config.toml")).unwrap();
        assert_eq!(
            format!(
                "extra = {}/extra_configs/extra.txt\n",
                expected_agent_dir.to_string_lossy()
            ),
            config_content
        );

        let extra_content =
            std::fs::read_to_string(expected_agent_dir.join("extra_configs").join("extra.txt"))
                .unwrap();
        assert_eq!("SOME CONTENT\n", extra_content);
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
