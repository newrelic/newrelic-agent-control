use super::values::values_repository::ValuesRepository;
use super::values::ValuesRepositoryError;
use crate::agent_type::agent_attributes::AgentAttributes;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use crate::agent_type::definition::{AgentType, AgentTypeDefinition};
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::agent_type::environment::Environment;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::renderer::{Renderer, TemplateRenderer};
use crate::agent_type::runtime_config::{Deployment, Runtime};
use crate::sub_agent::persister::config_persister_file::ConfigurationPersisterFile;
use crate::super_agent::config::{AgentID, SubAgentConfig};
use fs::file_reader::FileReaderError;
use std::fmt::Display;
use std::sync::Arc;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    #[error("error assembling agents: `{0}`")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("error assembling agents: `{0}`")]
    FileError(#[from] FileReaderError),
    #[error("error assembling agents: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("error assembling agents: `{0}`")]
    AgentTypeError(#[from] AgentTypeError),
    #[error("error assembling agents: `{0}`")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("values error: `{0}`")]
    ValuesRepositoryError(#[from] ValuesRepositoryError),
}

#[derive(Error, Debug)]
pub enum AgentTypeDefinitionError {
    #[error("invalid agent-type for `{0}` environment: `{1}")]
    EnvironmentError(AgentTypeError, Environment),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveAgent {
    agent_id: AgentID,
    runtime_config: Runtime,
}

impl Display for EffectiveAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.agent_id.as_ref().to_string_lossy())
    }
}

impl EffectiveAgent {
    pub(crate) fn new(agent_id: AgentID, runtime_config: Runtime) -> Self {
        Self {
            agent_id,
            runtime_config,
        }
    }

    pub(crate) fn get_runtime_config(&self) -> &Runtime {
        &self.runtime_config
    }

    #[allow(dead_code)]
    pub(crate) fn get_agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

pub trait EffectiveAgentsAssembler {
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

pub struct LocalEffectiveAgentsAssembler<R, D, N>
where
    R: AgentRegistry,
    D: ValuesRepository,
    N: Renderer,
{
    registry: R,
    values_repository: Arc<D>,
    renderer: N,
}

impl<D>
    LocalEffectiveAgentsAssembler<EmbeddedRegistry, D, TemplateRenderer<ConfigurationPersisterFile>>
where
    D: ValuesRepository,
{
    pub fn new(values_repository: Arc<D>) -> Self {
        LocalEffectiveAgentsAssembler {
            registry: EmbeddedRegistry::default(),
            values_repository,
            renderer: TemplateRenderer::default(),
        }
    }
}

impl<R, D, N> LocalEffectiveAgentsAssembler<R, D, N>
where
    R: AgentRegistry,
    D: ValuesRepository,
    N: Renderer,
{
    pub fn with_renderer(self, renderer: N) -> Self {
        Self { renderer, ..self }
    }
}

impl<R, D, N> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, D, N>
where
    R: AgentRegistry,
    D: ValuesRepository,
    N: Renderer,
{
    /// Load an agent type from the registry and populate it with values
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
        environment: &Environment,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load the agent type definition
        let agent_type_definition = self.registry.get(&agent_cfg.agent_type)?;
        // Build the corresponding agent type
        let agent_type = build_agent_type(agent_type_definition, environment)?;

        // Load the values
        let values = self.values_repository.load(agent_id, &agent_type)?;

        // Build the agent attributes
        let attributes = AgentAttributes {
            agent_id: agent_id.get(),
        };

        let runtime_config = self
            .renderer
            .render(agent_id, agent_type, values, attributes)?;

        Ok(EffectiveAgent::new(agent_id.clone(), runtime_config))
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
        definition.metadata,
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
    use crate::agent_type::agent_metadata::AgentMetadata;
    use crate::agent_type::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::agent_type::agent_values::AgentValues;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::renderer::tests::MockRendererMock;
    use crate::agent_type::runtime_config;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};
    use semver::Version;

    mock! {
        pub(crate) EffectiveAgentAssemblerMock {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssemblerMock {
            fn assemble_agent(
                &self,
                agent_id: &AgentID,
                agent_cfg: &SubAgentConfig,
                environment: &Environment,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
        }
    }

    impl MockEffectiveAgentAssemblerMock {
        pub fn should_assemble_agent(
            &mut self,
            agent_id: &AgentID,
            agent_cfg: &SubAgentConfig,
            environment: &Environment,
            effective_agent: EffectiveAgent,
        ) {
            self.expect_assemble_agent()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_cfg.clone()),
                    predicate::eq(environment.clone()),
                )
                .returning(move |_, _, _| Ok(effective_agent.clone()));
        }
    }

    impl<R, D, N> LocalEffectiveAgentsAssembler<R, D, N>
    where
        R: AgentRegistry,
        D: ValuesRepository,
        N: Renderer,
    {
        pub fn new_for_testing(registry: R, remote_values_repo: D, renderer: N) -> Self {
            Self {
                registry,
                values_repository: Arc::new(remote_values_repo),
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
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let environment = Environment::OnHost;
        let agent_type_definition = AgentTypeDefinition::empty_with_metadata(AgentMetadata {
            name: "some_fqn".into(),
            version: Version::parse("0.0.1").unwrap(),
            namespace: "ns".into(),
        });
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let values = AgentValues::default();
        let sub_agent_config = SubAgentConfig {
            agent_type: "ns/some_fqn:0.0.1".try_into().unwrap(),
        };
        let attributes = testing_agent_attributes(&agent_id);
        let rendered_runtime_config = testing_rendered_runtime_config();

        //Expectations
        registry.should_get("ns/some_fqn:0.0.1".to_string(), &agent_type_definition);

        sub_agent_values_repo.should_load(&agent_id, &agent_type, &values);
        renderer.should_render(
            &agent_id,
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
            .assemble_agent(&agent_id, &sub_agent_config, &environment)
            .unwrap();

        assert_eq!(rendered_runtime_config, effective_agent.runtime_config);
        assert_eq!(agent_id, effective_agent.agent_id);
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "namespace/some_fqn:0.0.1".try_into().unwrap(),
        };

        //Expectations
        registry.should_not_get("namespace/some_fqn:0.0.1".to_string());

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config, &Environment::OnHost);

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `agent not found`",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_assemble_agents_error_loading_values() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let environment = Environment::OnHost;
        let agent_type_definition = AgentTypeDefinition::empty_with_metadata(AgentMetadata {
            name: "some_fqn".into(),
            version: Version::parse("0.0.1").unwrap(),
            namespace: "ns".into(),
        });
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "ns/some_fqn:0.0.1".try_into().unwrap(),
        };

        //Expectations
        registry.should_get("ns/some_fqn:0.0.1".to_string(), &agent_type_definition);
        sub_agent_values_repo.should_not_load(&agent_id, &agent_type);

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config, &environment);

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
  runtime:
    on_host:
      executables:
        - path: /some/path
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
  runtime:
"#;
}
