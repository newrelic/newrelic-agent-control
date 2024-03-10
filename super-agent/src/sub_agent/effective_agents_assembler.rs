use std::fmt::Display;
use std::path::PathBuf;

use thiserror::Error;
use tracing::error;

use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError, LocalRegistry};
use crate::agent_type::definition::{AgentAttributes, AgentType, AgentTypeDefinition};
use crate::agent_type::environment::Environment;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::renderer::{Renderer, TemplateRenderer};
use crate::agent_type::runtime_config::Runtime;
use crate::sub_agent::values::values_repository::{
    ValuesRepository, ValuesRepositoryError, ValuesRepositoryFile,
};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::super_agent::defaults::{GENERATED_FOLDER_NAME, SUPER_AGENT_DATA_DIR};

use fs::{directory_manager::DirectoryManagerFs, file_reader::FileReaderError, LocalFile};

use super::persister::config_persister_file::ConfigurationPersisterFile;

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
    #[error("values error: `{0}`")]
    ValuesRepositoryError(#[from] ValuesRepositoryError),
}

#[derive(Clone)]
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
    values_repository: D,
    remote_enabled: bool,
    local_conf_path: Option<String>,
    renderer: N,
}

/// Type alias for the LocalEffectiveAgentsAssembler using local registry and files for values repository and configuration persistence.
pub type LocalSubAgentsAssembler = LocalEffectiveAgentsAssembler<
    LocalRegistry,
    ValuesRepositoryFile<LocalFile, DirectoryManagerFs>,
    TemplateRenderer<ConfigurationPersisterFile>,
>;

impl Default for LocalSubAgentsAssembler {
    fn default() -> Self {
        LocalEffectiveAgentsAssembler {
            registry: LocalRegistry::default(),
            values_repository: ValuesRepositoryFile::default().with_remote(),
            remote_enabled: false,
            local_conf_path: None,
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
    pub fn with_remote(self) -> LocalEffectiveAgentsAssembler<R, D, N> {
        Self {
            remote_enabled: true,
            ..self
        }
    }

    pub fn with_values_repository(self, values_repository: D) -> Self {
        Self {
            values_repository,
            ..self
        }
    }

    pub fn with_renderer(self, renderer: N) -> Self {
        Self { renderer, ..self }
    }

    pub fn build_absolute_path(&self, path: Option<&String>, agent_id: &AgentID) -> PathBuf {
        let base_data_dir = match path {
            Some(p) => p,
            None => SUPER_AGENT_DATA_DIR,
        };
        PathBuf::from(format!(
            "{}/{}/{}",
            base_data_dir, GENERATED_FOLDER_NAME, agent_id
        ))
    }

    #[cfg(feature = "custom-local-path")]
    pub fn with_base_dir(self, base_dir: &str) -> Self {
        Self {
            local_conf_path: Some(format!("{}{}", base_dir, SUPER_AGENT_DATA_DIR)),
            ..self
        }
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

        // Delete remote values if not supported
        if !self.remote_enabled || !agent_type.has_remote_management() {
            self.values_repository.delete_remote(agent_id)?;
        }

        // Load the values
        let values = self.values_repository.load(agent_id, &agent_type)?;

        // Build the agent attributes
        let attributes = AgentAttributes {
            // This is needed to create path for "file" variables, not used in k8s.
            generated_configs_path: self
                .build_absolute_path(self.local_conf_path.as_ref(), agent_id),
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
    _environment: &Environment,
) -> Result<AgentType, EffectiveAgentsAssemblerError> {
    // TODO: the [AgentType] variables will be different depending on the provided environment.
    // This could return an error if the agent type is not correct, given the provided environment.
    Ok(AgentType::new(
        definition.metadata,
        definition.variables,
        definition.runtime_config,
    ))
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {
    use mockall::{mock, predicate};

    use crate::agent_type::agent_type_registry::tests::MockAgentRegistryMock;
    use crate::agent_type::agent_values::AgentValues;
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::renderer::tests::MockRendererMock;
    use crate::agent_type::runtime_config;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;

    use super::*;

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
            efective_agent: EffectiveAgent,
        ) {
            self.expect_assemble_agent()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_cfg.clone()),
                    predicate::eq(environment.clone()),
                )
                .returning(move |_, _, _| Ok(efective_agent.clone()));
        }
    }

    impl<R, D, N> LocalEffectiveAgentsAssembler<R, D, N>
    where
        R: AgentRegistry,
        D: ValuesRepository,
        N: Renderer,
    {
        pub fn new_for_testing(
            registry: R,
            remote_values_repo: D,
            renderer: N,
            opamp_enabled: bool,
        ) -> Self {
            Self {
                registry,
                values_repository: remote_values_repo,
                remote_enabled: opamp_enabled,
                renderer,
                local_conf_path: None,
            }
        }
    }

    // Returns a testing runtime config with some content.
    fn testing_rendered_runtime_config() -> Runtime {
        Runtime {
            deployment: runtime_config::Deployment {
                on_host: None,
                k8s: Some(runtime_config::K8s {
                    objects: vec![("key".to_string(), runtime_config::K8sObject::default())]
                        .into_iter()
                        .collect(),
                }),
            },
        }
    }

    // Returns the expected agent_attributes given an agent_id.
    fn testing_agent_attributes(agent_id: &AgentID) -> AgentAttributes {
        AgentAttributes {
            agent_id: agent_id.to_string(),
            generated_configs_path: PathBuf::from(format!(
                "/var/lib/newrelic-super-agent/auto-generated/{}",
                agent_id,
            )),
        }
    }

    #[test]
    fn test_assemble_agents_opamp_disabled() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let environment = Environment::OnHost;
        let agent_type_definition = AgentTypeDefinition::default();
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let values = AgentValues::default();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };
        let attributes = testing_agent_attributes(&agent_id);
        let rendered_runtime_config = testing_rendered_runtime_config();

        //Expectations
        registry.should_get("some_fqn".to_string(), &agent_type_definition);
        //Delete remote as opamp is disabled
        sub_agent_values_repo.should_delete_remote(&agent_id);
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
            false,
        );

        let effective_agent = assembler
            .assemble_agent(&agent_id, &sub_agent_config, &environment)
            .unwrap();

        assert_eq!(rendered_runtime_config, effective_agent.runtime_config);
        assert_eq!(agent_id, effective_agent.agent_id);
    }

    #[test]
    fn test_assemble_agents_opamp_enabled() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let environment = Environment::OnHost;
        let agent_type_definition = AgentTypeDefinition::default();
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let values = AgentValues::default();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };
        let attributes = testing_agent_attributes(&agent_id);
        let rendered_runtime_config = testing_rendered_runtime_config();

        //Expectations
        registry.should_get("some_fqn".to_string(), &agent_type_definition);
        // Opamp is enabled, so we expect to load values
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
            true,
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
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_not_get("some_fqn".to_string());

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config, &Environment::OnHost);

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `agent not found`",
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_assemble_agents_error_deleting_remote() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let renderer = MockRendererMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let environment = Environment::OnHost;
        let agent_type_definition = AgentTypeDefinition::default();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &agent_type_definition);
        sub_agent_values_repo.should_not_delete_remote(&agent_id);

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config, &environment);

        assert!(result.is_err());
        assert_eq!(
            "values error: `incorrect path`",
            result.err().unwrap().to_string()
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
        let agent_type_definition = AgentTypeDefinition::default();
        let agent_type = build_agent_type(agent_type_definition.clone(), &environment).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &agent_type_definition);
        sub_agent_values_repo.should_delete_remote(&agent_id);
        sub_agent_values_repo.should_not_load(&agent_id, &agent_type);

        let assembler = LocalEffectiveAgentsAssembler::new_for_testing(
            registry,
            sub_agent_values_repo,
            renderer,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config, &environment);

        assert!(result.is_err());
        assert_eq!(
            "values error: `incorrect path`",
            result.err().unwrap().to_string()
        );
    }
}
