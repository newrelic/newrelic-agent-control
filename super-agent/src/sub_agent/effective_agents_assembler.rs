use std::fmt::Display;

use thiserror::Error;
use tracing::error;

use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError, LocalRegistry};
use crate::agent_type::definition::AgentAttributes;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::Runtime;
use crate::sub_agent::values::values_repository::{
    ValuesRepository, ValuesRepositoryError, ValuesRepositoryFile,
};
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::super_agent::defaults::{GENERATED_FOLDER_NAME, SUPER_AGENT_DATA_DIR};
use crate::super_agent::super_agent::EffectiveAgentsError;

use fs::{directory_manager::DirectoryManagerFs, file_reader::FileReaderError, LocalFile};

use super::persister::config_persister::{ConfigurationPersister, PersistError};
use super::persister::config_persister_file::ConfigurationPersisterFile;

#[derive(Error, Debug)]
pub enum EffectiveAgentsAssemblerError {
    #[error("error assembling agents: `{0}`")]
    ConfigurationPersisterError(#[from] PersistError),
    #[error("error assembling agents: `{0}`")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("error assembling agents: `{0}`")]
    FileError(#[from] FileReaderError),
    #[error("error assembling agents: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("error assembling agents: `{0}`")]
    AgentTypeError(#[from] AgentTypeError),
    #[error("error assembling agents: `{0}`")]
    EffectiveAgentsError(#[from] EffectiveAgentsError),
    #[error("could not get path string")]
    BadPath,
    #[error("cannot load remote config: `{0}`")]
    RemoteConfigLoadError(String),
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
}

pub trait EffectiveAgentsAssembler {
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
}

pub struct LocalEffectiveAgentsAssembler<R, C, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    D: ValuesRepository,
{
    registry: R,
    config_persister: C,
    values_repository: D,
    remote_enabled: bool,
    local_conf_path: Option<String>,
}

impl Default
    for LocalEffectiveAgentsAssembler<
        LocalRegistry,
        ConfigurationPersisterFile,
        ValuesRepositoryFile<LocalFile, DirectoryManagerFs>,
    >
{
    fn default() -> Self {
        LocalEffectiveAgentsAssembler {
            registry: LocalRegistry::default(),
            config_persister: ConfigurationPersisterFile::default(),
            values_repository: ValuesRepositoryFile::default().with_remote(),
            remote_enabled: false,
            local_conf_path: None,
        }
    }
}

impl<R, C, D> LocalEffectiveAgentsAssembler<R, C, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    D: ValuesRepository,
{
    pub fn with_remote(mut self) -> LocalEffectiveAgentsAssembler<R, C, D> {
        self.remote_enabled = true;
        self
    }

    pub fn with_values_repository(mut self, values_repository: D) -> Self {
        self.values_repository = values_repository;
        self
    }

    pub fn with_config_persister(mut self, config_persister: C) -> Self {
        self.config_persister = config_persister;
        self
    }

    pub fn build_absolute_path(&self, path: Option<&String>, agent_id: &AgentID) -> String {
        let base_data_dir = match path {
            Some(p) => p,
            None => SUPER_AGENT_DATA_DIR,
        };
        format!("{}/{}/{}", base_data_dir, GENERATED_FOLDER_NAME, agent_id)
    }

    #[cfg(feature = "custom-local-path")]
    pub fn with_base_dir(mut self, base_dir: &str) -> Self {
        self.local_conf_path = Some(format!("{}{}", base_dir, SUPER_AGENT_DATA_DIR));
        self
    }
}

impl<R, C, D> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, C, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    D: ValuesRepository,
{
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError> {
        // Load agent type from repository and populate with values
        let final_agent = self.registry.get(&agent_cfg.agent_type)?;

        // delete remote values if not supported
        if !self.remote_enabled || !final_agent.has_remote_management() {
            self.values_repository.delete_remote(agent_id)?;
        }

        let agent_values = self.values_repository.load(agent_id, &final_agent)?;

        let absolute_path = self.build_absolute_path(self.local_conf_path.as_ref(), agent_id);

        let agent_attributes = AgentAttributes {
            configs_path: Some(absolute_path.as_str()),
            agent_id: agent_id.get(),
        };

        // populate with values
        let populated_agent = final_agent.template_with(agent_values, Some(agent_attributes))?;

        // clean existing config files if any
        self.config_persister
            .delete_agent_config(agent_id, &populated_agent)?;

        // persist config if agent requires it
        self.config_persister
            .persist_agent_config(agent_id, &populated_agent)?;

        Ok(EffectiveAgent::new(
            agent_id.clone(),
            populated_agent.runtime_config,
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {
    use mockall::{mock, predicate};
    use std::io::ErrorKind;

    use crate::agent_type::agent_values::AgentValues;
    use crate::agent_type::definition::AgentType;
    use crate::agent_type::runtime_config::Args;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::{
        agent_type::agent_type_registry::tests::MockAgentRegistryMock,
        sub_agent::persister::config_persister::test::MockConfigurationPersisterMock,
    };
    use fs::{directory_manager::DirectoryManagementError, writer_file::WriteError};

    use super::*;

    mock! {
        pub(crate) EffectiveAgentAssemblerMock {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssemblerMock {
            fn assemble_agent(
                &self,
                agent_id: &AgentID,
                agent_cfg: &SubAgentConfig,
            ) -> Result<EffectiveAgent, EffectiveAgentsAssemblerError>;
        }
    }

    impl MockEffectiveAgentAssemblerMock {
        pub fn should_assemble_agent(
            &mut self,
            agent_id: &AgentID,
            agent_cfg: &SubAgentConfig,
            efective_agent: EffectiveAgent,
        ) {
            self.expect_assemble_agent()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_cfg.clone()),
                )
                .returning(move |_, _| Ok(efective_agent.clone()));
        }

        #[allow(dead_code)]
        pub fn should_not_assemble_agent(
            &mut self,
            agent_id: &AgentID,
            agent_cfg: &SubAgentConfig,
            err_kind: ErrorKind,
        ) {
            self.expect_assemble_agent()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_cfg.clone()),
                )
                .returning(move |_, _| {
                    Err(EffectiveAgentsAssemblerError::ConfigurationPersisterError(
                        PersistError::FileError(WriteError::ErrorCreatingFile(
                            std::io::Error::from(err_kind),
                        )),
                    ))
                });
        }
    }

    impl<R, C, D> LocalEffectiveAgentsAssembler<R, C, D>
    where
        R: AgentRegistry,
        C: ConfigurationPersister,
        D: ValuesRepository,
    {
        pub fn new(
            registry: R,
            config_persister: C,
            remote_values_repo: D,
            opamp_enabled: bool,
        ) -> Self {
            Self {
                registry,
                config_persister,
                values_repository: remote_values_repo,
                remote_enabled: opamp_enabled,
                local_conf_path: None,
            }
        }
    }

    #[test]
    fn test_assemble_agents_opamp_disabled() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let agent_values: AgentValues = serde_yaml::from_reader(AGENT_VALUES.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        //Delete remote as opamp is disabled
        sub_agent_values_repo.should_delete_remote(&agent_id);
        sub_agent_values_repo.should_load(&agent_id, &final_agent, &agent_values);
        //From now on the EffectiveAgent is populated
        let populated_agent = final_agent
            .template_with(agent_values.clone(), None)
            .unwrap();
        config_persister.should_delete_agent_config(&agent_id, &populated_agent);
        config_persister.should_persist_agent_config(&agent_id, &populated_agent);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            false,
        );

        let effective_agent = assembler
            .assemble_agent(&agent_id, &sub_agent_config)
            .unwrap();

        assert_eq!(
            Args("--config_path=/some/path/config".into()),
            effective_agent
                .runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables
                .first()
                .unwrap()
                .args
                .clone()
                .get()
        );
    }

    #[test]
    fn test_assemble_agents_opamp_enabled() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let agent_values: AgentValues = serde_yaml::from_reader(AGENT_VALUES.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        sub_agent_values_repo.should_load(&agent_id, &final_agent, &agent_values);
        //From now on the EffectiveAgent is populated
        let populated_agent = final_agent
            .template_with(agent_values.clone(), None)
            .unwrap();
        config_persister.should_delete_agent_config(&agent_id, &populated_agent);
        config_persister.should_persist_agent_config(&agent_id, &populated_agent);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            true,
        );

        let effective_agent = assembler
            .assemble_agent(&agent_id, &sub_agent_config)
            .unwrap();

        assert_eq!(
            Args("--config_path=/some/path/config".into()),
            effective_agent
                .runtime_config
                .deployment
                .on_host
                .unwrap()
                .executables
                .first()
                .unwrap()
                .args
                .clone()
                .get()
        );
    }

    #[test]
    fn test_assemble_agents_error_on_registry() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_not_get("some_fqn".to_string());

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config);

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
        let config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        //Delete remote as opamp is disabled
        sub_agent_values_repo.should_not_delete_remote(&agent_id);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config);

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
        let config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        sub_agent_values_repo.should_delete_remote(&agent_id);
        sub_agent_values_repo.should_not_load(&agent_id, &final_agent);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            false,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config);

        assert!(result.is_err());
        assert_eq!(
            "values error: `incorrect path`",
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_assemble_agents_error_deleting_persisted_config() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let agent_values: AgentValues = serde_yaml::from_reader(AGENT_VALUES.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        sub_agent_values_repo.should_load(&agent_id, &final_agent, &agent_values);
        //From now on the EffectiveAgent is populated
        let populated_agent = final_agent
            .template_with(agent_values.clone(), None)
            .unwrap();
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        config_persister.should_not_delete_agent_config(&agent_id, &populated_agent, err);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            true,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config);

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `directory error: `cannot delete directory: `oh no...```",
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn test_assemble_agents_error_persisting_config() {
        //Mocks
        let mut registry = MockAgentRegistryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut config_persister = MockConfigurationPersisterMock::new();

        // Objects
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let final_agent: AgentType = serde_yaml::from_reader(AGENT_TYPE.as_bytes()).unwrap();
        let agent_values: AgentValues = serde_yaml::from_reader(AGENT_VALUES.as_bytes()).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: "some_fqn".into(),
        };

        //Expectations
        registry.should_get("some_fqn".to_string(), &final_agent);
        sub_agent_values_repo.should_load(&agent_id, &final_agent, &agent_values);
        //From now on the EffectiveAgent is populated
        let populated_agent = final_agent
            .template_with(agent_values.clone(), None)
            .unwrap();
        config_persister.should_delete_agent_config(&agent_id, &populated_agent);
        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "oh no...".to_string(),
        ));
        config_persister.should_not_persist_agent_config(&agent_id, &populated_agent, err);

        let assembler = LocalEffectiveAgentsAssembler::new(
            registry,
            config_persister,
            sub_agent_values_repo,
            true,
        );

        let result = assembler.assemble_agent(&agent_id, &sub_agent_config);

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `directory error: `cannot delete directory: `oh no...```",
            result.err().unwrap().to_string()
        );
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Fixtures and helpers
    ////////////////////////////////////////////////////////////////////////////////////

    const AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  config_path:
    description: "config file string"
    type: string
    required: true
deployment:
  on_host:
    executables:
      - path: /opt/first 
        args: "--config_path=${nr-var:config_path}"
        env: ""
"#;

    const AGENT_VALUES: &str = r#"
config_path: /some/path/config
"#;
}
