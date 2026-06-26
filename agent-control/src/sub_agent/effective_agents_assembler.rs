//! Assembles an [EffectiveAgent] (rendered runtime configuration) from an agent identity and
//! its YAML config, resolving the agent type, variables, and secrets.

use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::render::TemplateRenderer;
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::rendered::OnHost;
use crate::agent_type::runtime_config::rendered;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::agent_type::variable::secret_variables::{
    SecretVariables, SecretVariablesError, load_env_vars,
};
use crate::secrets_provider::SecretsProviders;
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::YAMLConfig;

use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Errors produced while assembling an [EffectiveAgent].
#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    /// A generic assembly failure with a descriptive message.
    #[error("error assembling agents: {0}")]
    EffectiveAgentsAssemblerError(String),
    /// The agent type could not be retrieved from the registry.
    #[error("error assembling agents: {0}")]
    Registry(#[from] AgentTypeRegistryError),
    /// YAML (de)serialization failed.
    #[error("error assembling agents: {0}")]
    SerializationError(#[from] serde_saphyr::Error),
    /// A value could not be converted to/from JSON.
    #[error("error assembling agents: {0}")]
    ValueConversionError(#[from] serde_json::Error),
    /// The agent type definition was invalid.
    #[error("error assembling agents: {0}")]
    AgentTypeError(#[from] AgentTypeError),
    /// Secret variables could not be loaded.
    #[error("error loading secrets: {0}")]
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

    pub(crate) fn get_onhost_config(&self) -> Result<&OnHost, EffectiveAgentsAssemblerError> {
        match &self.runtime_config.deployment {
            rendered::Deployment::Host(on_host) => Ok(on_host),
            rendered::Deployment::K8s(_) => Err(
                EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                    "missing host deployment configuration".to_string(),
                ),
            ),
        }
    }

    pub(crate) fn get_k8s_config(&self) -> Result<&K8s, EffectiveAgentsAssemblerError> {
        match &self.runtime_config.deployment {
            rendered::Deployment::K8s(k8s) => Ok(k8s),
            rendered::Deployment::Host(_) => Err(
                EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                    "missing k8s deployment configuration".to_string(),
                ),
            ),
        }
    }

    pub(crate) fn get_agent_identity(&self) -> &AgentIdentity {
        &self.agent_identity
    }
}

impl TryFrom<EffectiveAgent> for K8s {
    type Error = EffectiveAgentsAssemblerError;

    fn try_from(value: EffectiveAgent) -> Result<Self, Self::Error> {
        match value.runtime_config.deployment {
            rendered::Deployment::K8s(k8s) => Ok(k8s),
            rendered::Deployment::Host(_) => Err(
                EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                    "missing k8s deployment configuration".to_string(),
                ),
            ),
        }
    }
}

/// Assembles an [EffectiveAgent] from an agent identity and its YAML configuration.
pub trait EffectiveAgentsAssembler {
    /// Assemble an [EffectiveAgent] from an [AgentIdentity]. The implementer is responsible for
    /// getting the AgentType and all needed values to render the Runtime config.
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        yaml_config: YAMLConfig,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

/// Implements [EffectiveAgentsAssembler] and is responsible for:
/// - Getting [`AgentType`](crate::agent_type::definition::AgentType) from [AgentTypeRegistry]
/// - Getting Local or Remote configs from [`ConfigRepository`](crate::values::config_repository::ConfigRepository)
/// - Rendering the [`Runtime`](crate::agent_type::runtime_config::Runtime) configuration of an Agent
///
/// Important: Assembling an Agent may mutate the state of external resources by creating
/// or removing configs when the `Runtime` is rendered.
pub struct LocalEffectiveAgentsAssembler<R>
where
    R: AgentTypeRegistry,
{
    registry: Arc<R>,
    renderer: TemplateRenderer,
    variable_constraints: VariableConstraints,
    secrets_providers: SecretsProviders,
    remote_dir: PathBuf,
}

impl<R> LocalEffectiveAgentsAssembler<R>
where
    R: AgentTypeRegistry,
{
    /// Creates an assembler from an agent-type registry, template renderer, variable constraints,
    /// secrets providers, and the remote configuration directory.
    pub fn new(
        registry: Arc<R>,
        renderer: TemplateRenderer,
        variable_constraints: VariableConstraints,
        secrets_providers: SecretsProviders,
        remote_dir: &Path,
    ) -> Self {
        LocalEffectiveAgentsAssembler {
            registry,
            renderer,
            variable_constraints,
            secrets_providers,
            remote_dir: remote_dir.to_path_buf(),
        }
    }
}

impl<R> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R>
where
    R: AgentTypeRegistry,
{
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        values: YAMLConfig,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the parsed definition and apply the AC-wide variable constraints to materialize
        // an [AgentType] ready for the renderer.
        let agent_type = self
            .registry
            .get(&agent_identity.agent_type_id)?
            .with_constraints(&self.variable_constraints);

        let attributes =
            AgentAttributes::try_new(agent_identity.id.to_owned(), self.remote_dir.to_path_buf())
                .map_err(|e| {
                EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(e.to_string())
            })?;

        // Values are expanded substituting all ${nr-env...} with environment variables.
        // Notice that only environment variables are taken into consideration (no other vars for example)
        let secret_variables = SecretVariables::try_from(values.clone())?;
        let env_vars = load_env_vars();
        let secrets = secret_variables.load_secrets(&self.secrets_providers)?;

        let runtime_config = self
            .renderer
            .render(agent_type, values, attributes, env_vars, secrets)?;

        Ok(EffectiveAgent::new(agent_identity.clone(), runtime_config))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[allow(missing_docs)] // test-support code
pub(crate) mod tests {

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use crate::values::yaml_config::YAMLConfig;
    use mockall::mock;

    mock! {
        pub EffectiveAgentAssembler {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssembler {
            fn assemble_agent(
                &self,
                agent_identity:&AgentIdentity,
                yaml_config: YAMLConfig,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;

        }
    }

    impl<R> LocalEffectiveAgentsAssembler<R>
    where
        R: AgentTypeRegistry,
    {
        pub fn new_for_testing(registry: R) -> Self {
            Self {
                registry: Arc::new(registry),
                renderer: TemplateRenderer::default(),
                variable_constraints: VariableConstraints::default(),
                secrets_providers: SecretsProviders::default(),
                remote_dir: PathBuf::default(),
            }
        }
    }

    #[test]
    fn test_assemble_agents() {
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

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry);

        let effective_agent = assembler.assemble_agent(&agent_identity, values).unwrap();

        assert_eq!(agent_identity, effective_agent.agent_identity);
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentTypeRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
        ));

        //Expectations
        registry.expect_get_not_found(AgentTypeID::try_from("namespace/name:0.0.1").unwrap());
        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry);

        let result = assembler.assemble_agent(&agent_identity, YAMLConfig::default());

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: agent type namespace/name:0.0.1 not found",
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
}
