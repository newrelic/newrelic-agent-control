use thiserror::Error;

use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::super_agent_configs::{AgentID, SubAgentConfig};
use crate::super_agent::super_agent::{EffectiveAgents, EffectiveAgentsError};
use crate::{
    config::{
        agent_type::error::AgentTypeError,
        agent_type_registry::{AgentRegistry, AgentRepositoryError, LocalRegistry},
        agent_values::AgentValues,
        persister::{
            config_persister::{ConfigurationPersister, PersistError},
            config_persister_file::ConfigurationPersisterFile,
        },
        super_agent_configs::SuperAgentConfig,
    },
    file_reader::{FSFileReader, FileReader, FileReaderError},
};

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
}

pub trait EffectiveAgentsAssembler {
    fn assemble_agents(
        &self,
        agent_cfgs: &SuperAgentConfig,
    ) -> Result<EffectiveAgents, EffectiveAgentsAssemblerError>;

    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<FinalAgent, EffectiveAgentsAssemblerError>;
}

pub struct LocalEffectiveAgentsAssembler<R: AgentRegistry, C: ConfigurationPersister, F: FileReader>
{
    registry: R,
    config_persister: C,
    file_reader: F,
}

impl Default
    for LocalEffectiveAgentsAssembler<LocalRegistry, ConfigurationPersisterFile, FSFileReader>
{
    fn default() -> Self {
        Self {
            registry: LocalRegistry::default(),
            config_persister: ConfigurationPersisterFile::default(),
            file_reader: FSFileReader,
        }
    }
}

impl<R, C, F> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, C, F>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    F: FileReader,
{
    fn assemble_agents(
        &self,
        agent_cfgs: &SuperAgentConfig,
    ) -> Result<EffectiveAgents, EffectiveAgentsAssemblerError> {
        //clean all temporary configurations
        self.config_persister.delete_all_configs()?;
        let mut effective_agents = EffectiveAgents::default();

        for (agent_id, agent_cfg) in agent_cfgs.agents.iter() {
            let effective_agent = self.assemble_agent(agent_id, agent_cfg)?;
            effective_agents.add(agent_id.clone(), effective_agent)?;
        }
        Ok(effective_agents)
    }

    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<FinalAgent, EffectiveAgentsAssemblerError> {
        //load agent type from repository and populate with values
        let agent_type = self.registry.get(&agent_cfg.agent_type)?;
        let mut agent_config: AgentValues = AgentValues::default();
        if let Some(path) = &agent_cfg.values_file {
            let contents = self.file_reader.read(path.as_str())?;
            agent_config = serde_yaml::from_str(&contents)?;
        }
        // populate with values
        let populated_agent = agent_type.clone().template_with(agent_config)?;

        // clean existing config files if any
        self.config_persister
            .delete_agent_config(agent_id, &populated_agent)?;

        // persist config if agent requires it
        self.config_persister
            .persist_agent_config(agent_id, &populated_agent)?;

        Ok(populated_agent)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use mockall::predicate;
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};

    use crate::{
        config::{
            agent_type::{agent_types::FinalAgent, trivial_value::TrivialValue},
            agent_type_registry::{AgentRegistry, LocalRegistry},
            agent_values::AgentValues,
            persister::{
                config_persister::{test::MockConfigurationPersisterMock, ConfigurationPersister},
                config_writer_file::WriteError,
                directory_manager::DirectoryManagementError,
            },
            super_agent_configs::{AgentID, SubAgentConfig, SuperAgentConfig},
        },
        file_reader::{test::MockFileReaderMock, FileReader},
    };

    use super::*;

    impl<R, C, F> LocalEffectiveAgentsAssembler<R, C, F>
    where
        R: AgentRegistry,
        C: ConfigurationPersister,
        F: FileReader,
    {
        pub fn new(registry: R, config_persister: C, file_reader: F) -> Self {
            Self {
                registry,
                config_persister,
                file_reader,
            }
        }
    }

    #[test]
    fn assemble_agents_test() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();

        file_reader_mock
            .expect_read()
            .with(predicate::eq("second.yaml".to_string()))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_delete_all_configs();

        // cannot assert on agent types as it's iterating a hashmap
        config_persister.should_delete_any_agent_config(2);
        config_persister.should_persist_any_agent_config(2);

        let effective_agents = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            file_reader_mock,
        )
        .assemble_agents(&agent_config)
        .unwrap();

        let first_agent = effective_agents.get(&AgentID::new("first")).unwrap();
        let file = first_agent
            .variables
            .get("config")
            .unwrap()
            .final_value
            .clone()
            .unwrap();
        let TrivialValue::File(f) = file else {
            unreachable!("Not a file")
        };
        assert_eq!("license_key: abc123\nstaging: true\n", f.content);
    }

    #[test]
    fn assemble_agents_fails_if_cannot_clean_folder() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        //not idempotent test as the order of a hashmap is random
        file_reader_mock.could_read("second.yaml".to_string(), SECOND_TYPE_VALUES.to_string());

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_delete_all_configs();

        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "unauthorized".to_string(),
        ));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_delete_any_agent_config(err);

        let result = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            file_reader_mock,
        )
        .assemble_agents(&agent_config);

        assert_eq!(true, result.is_err());
        assert_eq!(
            "error assembling agents: `directory error: `cannot delete directory: `unauthorized```"
                .to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn assemble_agents_fails_if_cannot_write_file() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        file_reader_mock.could_read("second.yaml".to_string(), SECOND_TYPE_VALUES.to_string());

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_delete_all_configs();

        config_persister.should_delete_any_agent_config(1);
        let err = PersistError::FileError(WriteError::ErrorCreatingFile(Error::from(
            ErrorKind::PermissionDenied,
        )));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_persist_any_agent_config(err);

        let result = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            file_reader_mock,
        )
        .assemble_agents(&agent_config);

        assert_eq!(true, result.is_err());
        assert_eq!(
            "error assembling agents: `file error: `error creating file: `permission denied```"
                .to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn empty_cfgs_test() {
        let agent_type_registry = LocalRegistry::default();
        let file_reader_mock = MockFileReaderMock::new();
        let agent_config = SuperAgentConfig {
            agents: Default::default(),
            opamp: None,
        };

        let mut config_persister = MockConfigurationPersisterMock::new();
        config_persister.should_delete_all_configs();

        let effective_agents = LocalEffectiveAgentsAssembler::new(
            agent_type_registry,
            config_persister,
            file_reader_mock,
        )
        .assemble_agents(&agent_config)
        .unwrap();

        let expected_effective_agents = EffectiveAgents::default();
        assert_eq!(expected_effective_agents, effective_agents);
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Fixtures and helpers
    ////////////////////////////////////////////////////////////////////////////////////

    const FIRST_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  config:
    description: "config file"
    type: file
    required: false
    default: |
        license_key: abc123
        staging: true
    file_path: some_file_name.yml
deployment:
  on_host:
    executables:
      - path: /opt/first
        args: "--config ${config}"
        env: ""
"#;

    const SECOND_TYPE: &str = r#"
namespace: newrelic
name: second
version: 0.1.0
variables:
  deployment:
    on_host:
      path:
        description: "Path to the agent"
        type: string
        required: true
      args:
        description: "Args passed to the agent"
        type: string
        required: false
        default: "an-arg"
deployment:
  on_host:
    executables:
      - path: ${deployment.on_host.path}/otelcol
        args: "-c ${deployment.on_host.args}"
        env: ""
"#;

    const SECOND_TYPE_VALUES: &str = r#"
deployment:
  on_host:
    path: another-path
"#;

    // not to copy and paste all the setup of the tests for load_agents_cnf
    fn load_agents_cnf_setup() -> (
        AgentID,
        AgentID,
        LocalRegistry,
        LocalRegistry,
        SuperAgentConfig,
    ) {
        let first_agent_id = AgentID("first".to_string());
        let second_agent_id = AgentID("second".to_string());
        let agent_types_and_values = vec![
            (first_agent_id.clone(), FIRST_TYPE, ""),
            (second_agent_id.clone(), SECOND_TYPE, SECOND_TYPE_VALUES),
        ];

        let mut local_agent_type_repository = LocalRegistry::default();

        // populate "repository" with unpopulated agent types
        agent_types_and_values
            .iter()
            .for_each(|(_, agent_type, _)| {
                let agent_type: FinalAgent =
                    serde_yaml::from_reader(agent_type.as_bytes()).unwrap();
                let res = local_agent_type_repository
                    .store_with_key(agent_type.metadata.to_string(), agent_type);
                assert_eq!(true, res.is_ok());
            });

        // just for the test
        let mut populated_agent_type_repository = LocalRegistry::default();
        // populate "repository" with unpopulated agent types
        agent_types_and_values
            .iter()
            .for_each(|(agent_id, agent_type, agent_values)| {
                let mut agent_type: FinalAgent =
                    serde_yaml::from_reader(agent_type.as_bytes()).unwrap();
                let agent_values: AgentValues =
                    serde_yaml::from_reader(agent_values.as_bytes()).unwrap();
                agent_type = agent_type.template_with(agent_values).unwrap();
                let res = populated_agent_type_repository
                    .store_with_key(agent_id.to_string(), agent_type);

                assert_eq!(true, res.is_ok());
            });

        let agent_config = SuperAgentConfig {
            agents: HashMap::from([
                (
                    first_agent_id.clone(),
                    SubAgentConfig {
                        agent_type: populated_agent_type_repository
                            .get(&first_agent_id)
                            .unwrap()
                            .metadata
                            .to_string()
                            .as_str()
                            .into(),
                        values_file: None,
                    },
                ),
                (
                    second_agent_id.clone(),
                    SubAgentConfig {
                        agent_type: populated_agent_type_repository
                            .get(&second_agent_id)
                            .unwrap()
                            .metadata
                            .to_string()
                            .as_str()
                            .into(),
                        values_file: Some("second.yaml".to_string()),
                    },
                ),
            ]),
            opamp: None,
        };

        (
            first_agent_id,
            second_agent_id,
            local_agent_type_repository,
            populated_agent_type_repository,
            agent_config,
        )
    }
}
