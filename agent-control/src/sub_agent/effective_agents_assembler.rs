use crate::agent_control::defaults::default_capabilities;
use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use crate::agent_type::definition::{AgentType, AgentTypeDefinition};
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::agent_type::environment::Environment;
use crate::agent_type::environment_variable::retrieve_env_var_variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::render::persister::config_persister_file::ConfigurationPersisterFile;
use crate::agent_type::render::renderer::{Renderer, TemplateRenderer};

use crate::agent_type::runtime_config::K8s;

use crate::agent_type::runtime_config::OnHost;
use crate::agent_type::runtime_config::{Deployment, Runtime};
use crate::sub_agent::identity::AgentIdentity;
use crate::values::yaml_config::YAMLConfig;
use crate::values::yaml_config_repository::{
    load_remote_fallback_local, YAMLConfigRepository, YAMLConfigRepositoryError,
};
use std::fmt::Display;
use std::sync::Arc;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    #[error("error assembling agents: `{0}`")]
    EffectiveAgentsAssemblerError(String),
    #[error("error assembling agents: `{0}`")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("error assembling agents: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("error assembling agents: `{0}`")]
    AgentTypeError(#[from] AgentTypeError),
    #[error("error assembling agents: `{0}`")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("values error: `{0}`")]
    YAMLConfigRepositoryError(#[from] YAMLConfigRepositoryError),
}

#[derive(Error, Debug)]
pub enum AgentTypeDefinitionError {
    #[error("invalid agent-type for `{0}` environment: `{1}")]
    EnvironmentError(AgentTypeError, Environment),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveAgent {
    agent_identity: AgentIdentity,
    runtime_config: Runtime,
}

impl Display for EffectiveAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.agent_identity.id.to_string())
    }
}

impl EffectiveAgent {
    pub(crate) fn new(agent_identity: AgentIdentity, runtime_config: Runtime) -> Self {
        Self {
            agent_identity,
            runtime_config,
        }
    }

    pub(crate) fn get_onhost_config(&self) -> Result<&OnHost, EffectiveAgentsAssemblerError> {
        self.runtime_config.deployment.on_host.as_ref().ok_or(
            EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(
                "missing on_host deployment configuration".to_string(),
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
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
    /// Perform the same operation as [assemble_agent] but using a provided config [YAMLConfig].
    fn assemble_agent_from_values(
        &self,
        config_values: YAMLConfig,
        agent_identity: &AgentIdentity,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

/// Implements [EffectiveAgentsAssembler] and is responsible for:
/// - Getting [AgentType] from [AgentRegistry]
/// - Getting Local or Remote configs from [YAMLConfigRepository]
/// - Rendering the [Runtime] configuration of an Agent
///
/// Important: Assembling an Agent may mutate the state of external resources by creating
/// or removing configs when the Runtime is [Renderer].
pub struct LocalEffectiveAgentsAssembler<R, D, Y>
where
    R: AgentRegistry,
    D: YAMLConfigRepository,
    Y: Renderer,
{
    registry: Arc<R>,
    yaml_config_repository: Arc<D>,
    renderer: Y,
}

impl<Y>
    LocalEffectiveAgentsAssembler<EmbeddedRegistry, Y, TemplateRenderer<ConfigurationPersisterFile>>
where
    Y: YAMLConfigRepository,
{
    pub fn new(
        yaml_config_repository: Arc<Y>,
        registry: Arc<EmbeddedRegistry>,
        renderer: TemplateRenderer<ConfigurationPersisterFile>,
    ) -> Self {
        LocalEffectiveAgentsAssembler {
            registry,
            yaml_config_repository,
            renderer,
        }
    }
}

impl<R, D, N> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, D, N>
where
    R: AgentRegistry,
    D: YAMLConfigRepository,
    N: Renderer,
{
    fn assemble_agent(
        &self,
        agent_identity: &AgentIdentity,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the values
        let values = load_remote_fallback_local(
            self.yaml_config_repository.as_ref(),
            &agent_identity.id,
            &default_capabilities(),
        )?;
        self.assemble_agent_from_values(values, agent_identity, environment)
    }

    fn assemble_agent_from_values(
        &self,
        values: YAMLConfig,
        agent_identity: &AgentIdentity,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the agent type definition
        let agent_type_definition = self
            .registry
            .get(&agent_identity.agent_type_id.to_string())?;
        // Build the corresponding agent type
        let agent_type = build_agent_type(agent_type_definition, environment)?;

        // Build the agent attributes
        let attributes = AgentAttributes {
            agent_id: agent_identity.id.get(),
        };

        // Values are expanded substituting all ${nr-env...} with environment variables.
        // Notice that only environment variables are taken into consideration (no other vars for example)
        let environment_variables = retrieve_env_var_variables();

        let runtime_config = self.renderer.render(
            &agent_identity.id,
            agent_type,
            values,
            attributes,
            environment_variables,
        )?;

        Ok(EffectiveAgent::new(agent_identity.clone(), runtime_config))
    }
}

/// Builds an [AgentType] given the provided [AgentTypeDefinition] and environment.
pub fn build_agent_type(
    definition: AgentTypeDefinition,
    environment: &Environment,
) -> Result<AgentType, AgentTypeDefinitionError> {
    // Select vars and runtime config according to the environment
    let (specific_vars, runtime_config) = match environment {
        Environment::K8s => (
            definition.variables.k8s,
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    ..definition.runtime_config.deployment
                },
            },
        ),
        Environment::OnHost => (
            definition.variables.on_host,
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
        .map_err(|err| AgentTypeDefinitionError::EnvironmentError(err, environment.clone()))?;

    Ok(AgentType::new(
        definition.agent_type_id,
        merged_variables,
        runtime_config,
    ))
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;

    use crate::agent_control::defaults::default_capabilities;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::render::renderer::tests::MockRendererMock;
    use crate::agent_type::runtime_config;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    mock! {
        pub(crate) EffectiveAgentAssemblerMock {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssemblerMock {
            fn assemble_agent(
                &self,
                agent_identity:&AgentIdentity,
                environment: &Environment,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
            fn assemble_agent_from_values(
                &self,
                values: YAMLConfig,
                agent_identity:&AgentIdentity,
                environment: &Environment,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;

        }
    }

    impl MockEffectiveAgentAssemblerMock {
        pub fn should_assemble_agent(
            &mut self,
            agent_identity: &AgentIdentity,
            environment: &Environment,
            effective_agent: EffectiveAgent,
            times: usize,
        ) {
            self.expect_assemble_agent()
                .times(times)
                .with(
                    predicate::eq(agent_identity.clone()),
                    predicate::eq(environment.clone()),
                )
                .returning(move |_, _| Ok(effective_agent.clone()));
        }
    }

    impl<R, D, N> LocalEffectiveAgentsAssembler<R, D, N>
    where
        R: AgentRegistry,
        D: YAMLConfigRepository,
        N: Renderer,
    {
        pub fn new_for_testing(registry: R, remote_values_repo: D, renderer: N) -> Self {
            Self {
                registry: Arc::new(registry),
                yaml_config_repository: Arc::new(remote_values_repo),
                renderer,
            }
        }
    }

    // Returns a testing runtime config with some content.
    fn testing_rendered_runtime_config() -> Runtime {
        Runtime {
            deployment: Deployment {
                on_host: None,
                k8s: Some(runtime_config::K8s {
                    objects: vec![("key".to_string(), runtime_config::K8sObject::default())]
                        .into_iter()
                        .collect(),
                    health: Some(Default::default()),
                }),
            },
        }
    }

    // Returns the expected agent_attributes given an agent_id.
    fn testing_agent_attributes(agent_id: &AgentID) -> AgentAttributes {
        AgentAttributes {
            agent_id: agent_id.to_string(),
        }
    }

    #[test]
    fn test_assemble_agents() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockYAMLConfigRepositoryMock::new();
        let mut renderer = MockRendererMock::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
        ));
        let environment = Environment::OnHost;
        let agent_type_definition =
            AgentTypeDefinition::empty_with_metadata("ns/name:0.0.1".try_into().unwrap());
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let values = YAMLConfig::default();

        let attributes = testing_agent_attributes(&agent_identity.id);
        let rendered_runtime_config = testing_rendered_runtime_config();

        //Expectations
        registry.should_get("ns/name:0.0.1".to_string(), &agent_type_definition);

        sub_agent_values_repo.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &values,
        );
        renderer.should_render(
            &agent_identity.id,
            &agent_type,
            &values,
            &attributes,
            rendered_runtime_config.clone(),
        );

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
        );

        let effective_agent = assembler
            .assemble_agent(&agent_identity, &environment)
            .unwrap();

        assert_eq!(rendered_runtime_config, effective_agent.runtime_config);
        assert_eq!(agent_identity, effective_agent.agent_identity);
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockYAMLConfigRepositoryMock::new();
        let renderer = MockRendererMock::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
        ));

        //Expectations
        registry.should_not_get("namespace/name:0.0.1".to_string());
        sub_agent_values_repo.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
        );

        let result = assembler.assemble_agent(&agent_identity, &Environment::OnHost);

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `agent type `namespace/name:0.0.1` not found`",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_assemble_agents_error_loading_values() {
        //Mocks
        let mut sub_agent_values_repo = MockYAMLConfigRepositoryMock::new();

        // Objects
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("ns/name:0.0.1").unwrap(),
        ));
        let environment = Environment::OnHost;

        //Expectations
        sub_agent_values_repo.should_not_load_remote(&agent_identity.id, default_capabilities());

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            MockAgentRegistryMock::new(),
            sub_agent_values_repo,
            MockRendererMock::new(),
        );

        let result = assembler.assemble_agent(&agent_identity, &environment);

        assert!(result.is_err());
    }

    #[test]
    fn test_build_agent_type() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(AGENT_TYPE_DEFINITION).unwrap();

        let k8s_agent_type = build_agent_type(definition.clone(), &Environment::K8s).unwrap();
        let k8s_vars = k8s_agent_type.variables.flatten();
        assert!(k8s_vars.contains_key("config.really_common"));
        let var = k8s_vars.get("config.var").unwrap();
        assert_eq!("K8s var".to_string(), var.description);
        assert!(
            k8s_agent_type.runtime_config.deployment.on_host.is_none(),
            "OnHost deployment for k8s should be none"
        );

        let on_host_agent_type = build_agent_type(definition, &Environment::OnHost).unwrap();
        let on_host_vars = on_host_agent_type.variables.flatten();
        assert!(on_host_vars.contains_key("config.really_common"));
        let var = on_host_vars.get("config.var").unwrap();
        assert_eq!("OnHost var".to_string(), var.description);
        assert!(
            on_host_agent_type.runtime_config.deployment.k8s.is_none(),
            "K8s deployment for on_host should be none"
        );
    }

    #[test]
    fn test_build_agent_type_error() {
        let definition =
            serde_yaml::from_str::<AgentTypeDefinition>(CONFLICTING_AGENT_TYPE_DEFINITION).unwrap();

        let expected_err = build_agent_type(definition, &Environment::K8s)
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
  on_host:
    config:
      var:
        description: "OnHost var"
        type: string
        required: true
deployment:
    on_host:
      executable:
        path: /some/path
        args: "${nr-var:config.really_common} ${config.var}"
    k8s:
      objects:
        chart:
          apiVersion: some.api.version/v1
          kind: SomeKind
          metadata:
            name: ${nr-sub:agent_id}
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
