use crate::agent_control::defaults::GENERATED_FOLDER_NAME;
use crate::agent_control::run::Environment;
use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use crate::agent_type::definition::{AgentType, AgentTypeDefinition};
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::render::TemplateRenderer;
use crate::agent_type::runtime_config::k8s::K8s;
use crate::agent_type::runtime_config::on_host::rendered::OnHost;
use crate::agent_type::runtime_config::{Deployment, Runtime, rendered};
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

#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    #[error("error assembling agents: {0}")]
    EffectiveAgentsAssemblerError(String),
    #[error("error assembling agents: {0}")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("error assembling agents: {0}")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("error assembling agents: {0}")]
    AgentTypeError(#[from] AgentTypeError),
    #[error("error assembling agents: {0}")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("error loading secrets: {0}")]
    SecretVariablesError(#[from] SecretVariablesError),
}

#[derive(Error, Debug)]
pub enum AgentTypeDefinitionError {
    #[error("invalid agent-type for '{0}' environment: {1}")]
    EnvironmentError(AgentTypeError, Environment),
}

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

    // Depending on the environment this method returns either the linux or windows deployment
    pub(crate) fn get_onhost_config(&self) -> Result<&OnHost, EffectiveAgentsAssemblerError> {
        #[cfg(target_family = "windows")]
        return self.runtime_config.deployment.windows.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing windows deployment configuration".to_string(),
            ),
        );
        #[cfg(target_family = "unix")]
        self.runtime_config.deployment.linux.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing linux deployment configuration".to_string(),
            ),
        )
    }

    pub(crate) fn get_k8s_config(&self) -> Result<&K8s, EffectiveAgentsAssemblerError> {
        self.runtime_config.deployment.k8s.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing k8s deployment configuration".to_string(),
            ),
        )
    }

    pub(crate) fn get_agent_identity(&self) -> &AgentIdentity {
        &self.agent_identity
    }
}

pub trait EffectiveAgentsAssembler {
    /// Assemble an [EffectiveAgent] from an [AgentIdentity]. The implementer is responsible for
    /// getting the AgentType and all needed values to render the Runtime config.
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        yaml_config: YAMLConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

/// Implements [EffectiveAgentsAssembler] and is responsible for:
/// - Getting [AgentType] from [AgentRegistry]
/// - Getting Local or Remote configs from [ConfigRepository]
/// - Rendering the [Runtime] configuration of an Agent
///
/// Important: Assembling an Agent may mutate the state of external resources by creating
/// or removing configs when the Runtime is [Renderer].
pub struct LocalEffectiveAgentsAssembler<R>
where
    R: AgentRegistry,
{
    registry: Arc<R>,
    renderer: TemplateRenderer,
    variable_constraints: VariableConstraints,
    secrets_providers: SecretsProviders,
    auto_generated_dir: PathBuf,
}

impl<R> LocalEffectiveAgentsAssembler<R>
where
    R: AgentRegistry,
{
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
            auto_generated_dir: remote_dir.join(GENERATED_FOLDER_NAME),
        }
    }
}

impl<R> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R>
where
    R: AgentRegistry,
{
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        values: YAMLConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the agent type definition
        let agent_type_definition = self
            .registry
            .get(&agent_identity.agent_type_id.to_string())?;
        // Build the corresponding agent type
        let agent_type = build_agent_type(
            agent_type_definition,
            environment,
            &self.variable_constraints,
        )?;

        // Build the agent attributes
        let attributes = AgentAttributes::try_new(
            agent_identity.id.to_owned(),
            self.auto_generated_dir.to_path_buf(),
        )
        .map_err(|e| EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(e.to_string()))?;

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

/// Builds an [AgentType] given the provided [AgentTypeDefinition] and environment.
pub fn build_agent_type(
    definition: AgentTypeDefinition,
    environment: &Environment,
    variable_constraints: &VariableConstraints,
) -> Result<AgentType, AgentTypeDefinitionError> {
    // Select vars and runtime config according to the environment
    let (specific_vars, runtime_config) = match environment {
        Environment::K8s => (
            definition.variables.k8s,
            Runtime {
                deployment: Deployment {
                    linux: None,
                    windows: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
        Environment::Linux => (
            definition.variables.linux,
            Runtime {
                deployment: Deployment {
                    k8s: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
        Environment::Windows => (
            definition.variables.windows,
            Runtime {
                deployment: Deployment {
                    k8s: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
    };
    // Merge common and specific variables
    let merged_variables = definition
        .variables
        .common
        .merge(specific_vars)
        .map_err(|err| AgentTypeDefinitionError::EnvironmentError(err, *environment))?;

    let agent_type_vars = merged_variables.with_config(variable_constraints);

    Ok(AgentType::new(
        definition.agent_type_id,
        agent_type_vars,
        runtime_config,
    ))
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {

    use super::*;
    use crate::agent_control::{agent_id::AgentID, run::on_host::AGENT_CONTROL_MODE_ON_HOST};
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistry;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::values::yaml_config::YAMLConfig;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    mock! {
        pub EffectiveAgentAssembler {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssembler {
            fn assemble_agent(
                &self,
                agent_identity:&AgentIdentity,
                yaml_config: YAMLConfig,
                environment: &Environment,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;

        }
    }

    impl MockEffectiveAgentAssembler {
        pub fn should_assemble_agent(
            &mut self,
            agent_identity: &AgentIdentity,
            yaml_config: &YAMLConfig,
            environment: &Environment,
            effective_agent: EffectiveAgent,
            times: usize,
        ) {
            self.expect_assemble_agent()
                .times(times)
                .with(
                    predicate::eq(agent_identity.clone()),
                    predicate::eq(yaml_config.clone()),
                    predicate::eq(*environment),
                )
                .returning(move |_, _, _| Ok(effective_agent.clone()));
        }
    }

    impl<R> LocalEffectiveAgentsAssembler<R>
    where
        R: AgentRegistry,
    {
        pub fn new_for_testing(registry: R) -> Self {
            Self {
                registry: Arc::new(registry),
                renderer: TemplateRenderer::default(),
                variable_constraints: VariableConstraints::default(),
                secrets_providers: SecretsProviders::default(),
                auto_generated_dir: PathBuf::default(),
            }
        }
    }

    #[test]
    fn test_assemble_agents() {
        // Mocks
        let mut registry = MockAgentRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
        ));
        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());
        let values = YAMLConfig::default();

        //Expectations
        registry.should_get("ns/name:0.0.1".to_string(), &agent_type_definition);

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry);

        let effective_agent = assembler
            .assemble_agent(&agent_identity, values, &AGENT_CONTROL_MODE_ON_HOST)
            .unwrap();

        assert_eq!(agent_identity, effective_agent.agent_identity);
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentRegistry::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
        ));

        //Expectations
        registry.should_not_get("namespace/name:0.0.1".to_string());
        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(registry);

        let result = assembler.assemble_agent(
            &agent_identity,
            YAMLConfig::default(),
            &AGENT_CONTROL_MODE_ON_HOST,
        );

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: agent type namespace/name:0.0.1 not found",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_build_agent_type() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(AGENT_TYPE_DEFINITION).unwrap();

        let k8s_agent_type = build_agent_type(
            definition.clone(),
            &Environment::K8s,
            &VariableConstraints::default(),
        )
        .unwrap();
        let k8s_vars = k8s_agent_type.variables.flatten();
        assert!(k8s_vars.contains_key("config.really_common"));
        let var = k8s_vars.get("config.var").unwrap();
        assert_eq!("K8s var".to_string(), var.description);
        assert!(
            k8s_agent_type.runtime_config.deployment.linux.is_none(),
            "linux deployment for k8s should be none"
        );
        assert!(
            k8s_agent_type.runtime_config.deployment.windows.is_none(),
            "windows deployment for k8s should be none"
        );

        let on_host_agent_type = build_agent_type(
            definition,
            &AGENT_CONTROL_MODE_ON_HOST,
            &VariableConstraints::default(),
        )
        .unwrap();
        let on_host_vars = on_host_agent_type.variables.flatten();
        assert!(on_host_vars.contains_key("config.really_common"));
        let var = on_host_vars.get("config.var").unwrap();
        #[cfg(target_family = "unix")]
        assert_eq!("Linux var".to_string(), var.description);
        #[cfg(target_family = "windows")]
        assert_eq!("Windows var".to_string(), var.description);
        assert!(
            on_host_agent_type.runtime_config.deployment.k8s.is_none(),
            "K8s deployment for on_host should be none"
        );
    }

    #[test]
    fn test_build_agent_type_error() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(CONFLICTING_AGENT_TYPE_DEFINITION).unwrap();

        let expected_err = build_agent_type(
            definition,
            &Environment::K8s,
            &VariableConstraints::default(),
        )
        .err()
        .unwrap();
        assert_matches!(expected_err, AgentTypeDefinitionError::EnvironmentError(err, env) => {
            assert_matches!(err, AgentTypeError::ConflictingVariableDefinition(key) => {
                assert_eq!("config.var".to_string(), key);
            });
            assert_matches!(env, Environment::K8s);
        });
    }

    const AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
variables:
  common:
    config:
      really_common:
        description: "Common var"
        type: string
        required: true
  k8s:
    config:
      var:
        description: "K8s var"
        type: string
        required: true
  linux:
    config:
      var:
        description: "Linux var"
        type: string
        required: true
  windows:
    config:
      var:
        description: "Windows var"
        type: string
        required: true
deployment:
    linux:
      executables:
        - id: my-exec
          path: /some/path
          args: "${nr-var:config.really_common} ${config.var}"
    windows:
      executables:
        - id: my-exec
          path: /some/path
          args: "${nr-var:config.really_common} ${config.var}"
    k8s:
      objects:
        chart:
          apiVersion: some.api.version/v1
          kind: SomeKind
          metadata:
            name: ${nr-sub:agent_id}
            namespace: ${nr-ac:namespace}
          spec:
            some_key: ${nr-var:config.really_common}
            other: ${nr-avar:config.var}
"#;

    const CONFLICTING_AGENT_TYPE_DEFINITION: &str = r#"
name: common
namespace: newrelic
version: 0.0.1
variables:
  common:
    config:
      var:
        description: "Common variable"
        type: string
        required: true
  k8s:
    config:
      var:
        description: "K8s variable"
        type: string
        required: true
deployment:
    k8s:
      objects: {}
"#;
}
