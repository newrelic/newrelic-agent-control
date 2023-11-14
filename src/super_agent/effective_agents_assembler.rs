use std::path::PathBuf;

use thiserror::Error;
use tracing::error;

use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::persister::config_writer_file::WriterFile;
use crate::config::persister::directory_manager::DirectoryManagerFs;
use crate::config::super_agent_configs::{
    get_remote_data_path, get_values_file_path, AgentID, SubAgentConfig,
};
use crate::sub_agent::values::remote_values_repository::{
    RemoteValuesRepository, RemoteValuesRepositoryError, RemoteValuesRepositoryFile,
};
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError::RemoteConfigLoadError;
use crate::super_agent::super_agent::EffectiveAgentsError;
use crate::{
    config::{
        agent_type::error::AgentTypeError,
        agent_type_registry::{AgentRegistry, AgentRepositoryError, LocalRegistry},
        agent_values::AgentValues,
        persister::{
            config_persister::{ConfigurationPersister, PersistError},
            config_persister_file::ConfigurationPersisterFile,
        },
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
    #[error("could not get path string")]
    BadPath,
    #[error("cannot load remote config: `{0}`")]
    RemoteConfigLoadError(String),
    #[error("values error: `{0}`")]
    ValuesRepositoryError(#[from] RemoteValuesRepositoryError),
}

pub trait EffectiveAgentsAssembler {
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<FinalAgent, EffectiveAgentsAssemblerError>;
}

pub struct LocalEffectiveAgentsAssembler<R, C, F, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    F: FileReader,
    D: RemoteValuesRepository,
{
    registry: R,
    config_persister: C,
    remote_values_repo: D,
    file_reader: F,
    opamp_enabled: bool,
}

impl
    LocalEffectiveAgentsAssembler<
        LocalRegistry,
        ConfigurationPersisterFile,
        FSFileReader,
        RemoteValuesRepositoryFile<DirectoryManagerFs, WriterFile>,
    >
{
    pub fn with_remote_management(
        opamp_enabled: bool,
    ) -> LocalEffectiveAgentsAssembler<
        LocalRegistry,
        ConfigurationPersisterFile,
        FSFileReader,
        RemoteValuesRepositoryFile<DirectoryManagerFs, WriterFile>,
    > {
        LocalEffectiveAgentsAssembler {
            registry: LocalRegistry::default(),
            config_persister: ConfigurationPersisterFile::default(),
            remote_values_repo: RemoteValuesRepositoryFile::default(),
            file_reader: FSFileReader,
            opamp_enabled,
        }
    }
}

impl<R, C, F, D> EffectiveAgentsAssembler for LocalEffectiveAgentsAssembler<R, C, F, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    F: FileReader,
    D: RemoteValuesRepository,
{
    fn assemble_agent(
        &self,
        agent_id: &AgentID,
        agent_cfg: &SubAgentConfig,
    ) -> Result<FinalAgent, EffectiveAgentsAssemblerError> {
        // Load agent type from repository and populate with values
        let final_agent = self.registry.get(&agent_cfg.agent_type)?;

        self.clean_values_if_not_accepting_remote(agent_id, &final_agent)?;

        let agent_config = self.load_values(agent_id, &final_agent)?;

        // populate with values
        let populated_agent = final_agent.template_with(agent_config)?;

        // clean existing config files if any
        self.config_persister
            .delete_agent_config(agent_id, &populated_agent)?;

        // persist config if agent requires it
        self.config_persister
            .persist_agent_config(agent_id, &populated_agent)?;

        Ok(populated_agent)
    }
}

impl<R, C, F, D> LocalEffectiveAgentsAssembler<R, C, F, D>
where
    R: AgentRegistry,
    C: ConfigurationPersister,
    F: FileReader,
    D: RemoteValuesRepository,
{
    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(
        &self,
        path: PathBuf,
    ) -> Result<Option<String>, EffectiveAgentsAssemblerError> {
        let remote_values_path = path
            .to_str()
            .ok_or(EffectiveAgentsAssemblerError::BadPath)?;
        let values_result = self.file_reader.read(remote_values_path);
        match values_result {
            Err(FileReaderError::FileNotFound(_)) => {
                //actively fallback to load local file
                Ok(None)
            }
            Ok(res) => Ok(Some(res)),
            Err(err) => {
                // we log any unexpected error for now but maybe we should propagate it
                error!("error loading remote file {}", remote_values_path);
                Err(err.into())
            }
        }
    }

    fn clean_values_if_not_accepting_remote(
        &self,
        agent_id: &AgentID,
        agent_type: &FinalAgent,
    ) -> Result<(), RemoteValuesRepositoryError> {
        let is_remote_capable = agent_type.has_remote_management();
        if !self.opamp_enabled || !is_remote_capable {
            //  clean up remote dirs
            return self.remote_values_repo.delete_agent_values(agent_id);
        }
        Ok(())
    }

    fn load_values(
        &self,
        agent_id: &AgentID,
        agent_type: &FinalAgent,
    ) -> Result<AgentValues, EffectiveAgentsAssemblerError> {
        let mut values_result: Result<Option<String>, EffectiveAgentsAssemblerError> = Ok(None);

        if self.opamp_enabled && agent_type.has_remote_management() {
            let remote_values_path = get_remote_data_path(agent_id).join("values.yml");
            values_result = self.load_file_if_present(remote_values_path);
            if let Err(e) = values_result {
                return Err(RemoteConfigLoadError(e.to_string()));
            }
        }

        if let Ok(None) = values_result {
            let local_values_path = get_values_file_path(agent_id);
            values_result = self.load_file_if_present(PathBuf::from(local_values_path));
        }

        match values_result {
            Ok(Some(contents)) => Ok(serde_yaml::from_str(&contents)?),
            Ok(None) => Ok(AgentValues::default()),
            Err(error) => Err(error),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub(crate) mod tests {
    use mockall::{mock, predicate};
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};

    use crate::sub_agent::values::remote_values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::super_agent::defaults::SUPER_AGENT_LOCAL_DATA_DIR;
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

    mock! {
        pub(crate) EffectiveAgentAssemblerMock {}

        impl EffectiveAgentsAssembler for EffectiveAgentAssemblerMock {
            fn assemble_agent(
                &self,
                agent_id: &AgentID,
                agent_cfg: &SubAgentConfig,
            ) -> Result<FinalAgent, EffectiveAgentsAssemblerError>;
        }
    }

    impl MockEffectiveAgentAssemblerMock {
        pub fn should_assemble_agent(
            &mut self,
            agent_id: &AgentID,
            agent_cfg: &SubAgentConfig,
            final_agent: FinalAgent,
        ) {
            self.expect_assemble_agent()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_cfg.clone()),
                )
                .returning(move |_, _| Ok(final_agent.clone()));
        }

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

    impl<R, C, F, D> LocalEffectiveAgentsAssembler<R, C, F, D>
    where
        R: AgentRegistry,
        C: ConfigurationPersister,
        F: FileReader,
        D: RemoteValuesRepository,
    {
        pub fn new(
            registry: R,
            config_persister: C,
            remote_config_persister: D,
            file_reader: F,
            opamp_enabled: bool,
        ) -> Self {
            Self {
                registry,
                config_persister,
                remote_values_repo: remote_config_persister,
                file_reader,
                opamp_enabled,
            }
        }
    }

    pub fn get_remote_values_file_path(agent_id: &AgentID) -> String {
        get_remote_data_path(agent_id)
            .join("values.yml")
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn assemble_agents_local_test() {
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
            .with(predicate::eq(get_remote_values_file_path(&_first_agent_id)))
            .times(1)
            .returning(|_| Err(FileReaderError::FileNotFound("file".to_string())));

        file_reader_mock
            .expect_read()
            .with(predicate::eq(get_values_file_path(&_first_agent_id)))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        file_reader_mock
            .expect_read()
            .with(predicate::eq(get_remote_values_file_path(
                &_second_agent_id,
            )))
            .times(1)
            .returning(|_| Err(FileReaderError::FileNotFound("file".to_string())));

        file_reader_mock
            .expect_read()
            .with(predicate::eq(get_values_file_path(&_second_agent_id)))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        let mut config_persister = MockConfigurationPersisterMock::new();

        // cannot assert on agent types as it's iterating a hashmap
        config_persister.should_delete_any_agent_config(2);
        config_persister.should_persist_any_agent_config(2);

        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        let assembler = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            sub_agent_values_repo,
            file_reader_mock,
            true,
        );

        let first_agent = assembler
            .assemble_agent(
                &AgentID::new("first").unwrap(),
                &agent_config
                    .agents
                    .get(&AgentID::new("first").unwrap())
                    .unwrap(),
            )
            .unwrap();

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

        let second_agent = assembler
            .assemble_agent(
                &AgentID::new("second").unwrap(),
                &agent_config
                    .agents
                    .get(&AgentID::new("second").unwrap())
                    .unwrap(),
            )
            .unwrap();

        let get_path = second_agent
            .variables
            .get("deployment.on_host.path")
            .unwrap()
            .final_value
            .clone()
            .unwrap();

        assert_eq!("another-path", get_path.to_string());
    }

    #[test]
    fn assemble_agents_remote_test() {
        // load the necessary objects for the test
        let (
            _first_agent_id,
            _second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            mut agent_config,
        ) = load_agents_cnf_setup();

        // Enable OpAMP for this test
        agent_config.opamp = Some(Default::default());

        let mut file_reader_mock = MockFileReaderMock::new();

        file_reader_mock
            .expect_read()
            .with(predicate::eq(get_remote_values_file_path(&_first_agent_id)))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        file_reader_mock
            .expect_read()
            .with(predicate::eq(get_remote_values_file_path(
                &_second_agent_id,
            )))
            .times(1)
            .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

        let mut config_persister = MockConfigurationPersisterMock::new();

        // cannot assert on agent types as it's iterating a hashmap
        config_persister.should_delete_any_agent_config(2);
        config_persister.should_persist_any_agent_config(2);

        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        let assembler = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            sub_agent_values_repo,
            file_reader_mock,
            true,
        );

        let first_agent = assembler
            .assemble_agent(
                &AgentID::new("first").unwrap(),
                &agent_config
                    .agents
                    .get(&AgentID::new("first").unwrap())
                    .unwrap(),
            )
            .unwrap();

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

        let second_agent = assembler
            .assemble_agent(
                &AgentID::new("second").unwrap(),
                &agent_config
                    .agents
                    .get(&AgentID::new("second").unwrap())
                    .unwrap(),
            )
            .unwrap();

        let get_path = second_agent
            .variables
            .get("deployment.on_host.path")
            .unwrap()
            .final_value
            .clone()
            .unwrap();

        assert_eq!("another-path", get_path.to_string());
    }

    // #[test]
    // fn assemble_agents_remote_test_no_capabilities() {
    //     // load the necessary objects for the test
    //     let (
    //         _first_agent_id,
    //         _second_agent_id,
    //         local_agent_type_repository,
    //         _populated_agent_type_repository,
    //         mut agent_config,
    //     ) = load_agents_cnf_setup();

    //     // Enable OpAMP for this test
    //     agent_config.opamp = Some(Default::default());

    //     // Disable remote config capabilities

    //     let mut file_reader_mock = MockFileReaderMock::new();

    //     file_reader_mock
    //         .expect_read()
    //         .with(predicate::eq(get_remote_values_file_path(&_first_agent_id)))
    //         .times(1)
    //         .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

    //     file_reader_mock
    //         .expect_read()
    //         .with(predicate::eq(get_remote_values_file_path(
    //             &_second_agent_id,
    //         )))
    //         .times(1)
    //         .returning(|_| Ok(SECOND_TYPE_VALUES.to_string()));

    //     let mut config_persister = MockConfigurationPersisterMock::new();
    //     config_persister.should_delete_all_configs();

    //     // cannot assert on agent types as it's iterating a hashmap
    //     config_persister.should_delete_any_agent_config(2);
    //     config_persister.should_persist_any_agent_config(2);

    //     let remote_config_persister = MockConfigurationPersisterMock::new();

    //     let effective_agents = LocalEffectiveAgentsAssembler::new(
    //         local_agent_type_repository,
    //         config_persister,
    //         remote_config_persister,
    //         file_reader_mock,
    //     )
    //     .assemble_agents(&agent_config)
    //     .unwrap();
    //     let first_agent = effective_agents.get(&AgentID::new("first")).unwrap();
    //     let file = first_agent
    //         .variables
    //         .get("config")
    //         .unwrap()
    //         .final_value
    //         .clone()
    //         .unwrap();
    //     let TrivialValue::File(f) = file else {
    //         unreachable!("Not a file")
    //     };
    //     assert_eq!("license_key: abc123\nstaging: true\n", f.content);

    //     let second_agent = effective_agents.get(&AgentID::new("second")).unwrap();
    //     let get_path = second_agent
    //         .variables
    //         .get("deployment.on_host.path")
    //         .unwrap()
    //         .final_value
    //         .clone()
    //         .unwrap();

    //     assert_eq!("another-path", get_path.to_string());
    // }

    #[test]
    fn assemble_agents_fails_if_cannot_clean_folder() {
        // load the necessary objects for the test
        let (
            first_agent_id,
            second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        //not idempotent test as the order of a hashmap is random
        file_reader_mock.could_read(
            format!("{SUPER_AGENT_LOCAL_DATA_DIR}fleet/agents.d/{first_agent_id}/values.yml"),
            SECOND_TYPE_VALUES.to_string(),
        );
        file_reader_mock.could_read(
            format!("{SUPER_AGENT_LOCAL_DATA_DIR}/fleet/agents.d/{second_agent_id}/values.yml"),
            SECOND_TYPE_VALUES.to_string(),
        );

        file_reader_mock.could_read(
            get_remote_values_file_path(&first_agent_id),
            SECOND_TYPE_VALUES.to_string(),
        );
        file_reader_mock.could_read(
            get_remote_values_file_path(&second_agent_id),
            SECOND_TYPE_VALUES.to_string(),
        );

        let mut config_persister = MockConfigurationPersisterMock::new();

        let err = PersistError::DirectoryError(DirectoryManagementError::ErrorDeletingDirectory(
            "unauthorized".to_string(),
        ));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_delete_any_agent_config(err);
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        let assembler = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            sub_agent_values_repo,
            file_reader_mock,
            true,
        );

        let result = assembler.assemble_agent(
            &AgentID::new("first").unwrap(),
            &agent_config
                .agents
                .get(&AgentID::new("first").unwrap())
                .unwrap(),
        );

        assert!(result.is_err());
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
            first_agent_id,
            second_agent_id,
            local_agent_type_repository,
            _populated_agent_type_repository,
            agent_config,
        ) = load_agents_cnf_setup();

        let mut file_reader_mock = MockFileReaderMock::new();
        file_reader_mock.could_read(
            format!("{SUPER_AGENT_LOCAL_DATA_DIR}/fleet/agents.d/{first_agent_id}/values.yml"),
            SECOND_TYPE_VALUES.to_string(),
        );
        file_reader_mock.could_read(
            format!("{SUPER_AGENT_LOCAL_DATA_DIR}/fleet/agents.d/{second_agent_id}/values.yml"),
            SECOND_TYPE_VALUES.to_string(),
        );

        file_reader_mock.could_read(
            get_remote_values_file_path(&first_agent_id),
            SECOND_TYPE_VALUES.to_string(),
        );
        file_reader_mock.could_read(
            get_remote_values_file_path(&second_agent_id),
            SECOND_TYPE_VALUES.to_string(),
        );

        let mut config_persister = MockConfigurationPersisterMock::new();

        config_persister.should_delete_any_agent_config(1);
        let err = PersistError::FileError(WriteError::ErrorCreatingFile(Error::from(
            ErrorKind::PermissionDenied,
        )));
        // we cannot assert on the agent as the order of a hashmap is random
        config_persister.should_not_persist_any_agent_config(err);
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        let assembler = LocalEffectiveAgentsAssembler::new(
            local_agent_type_repository,
            config_persister,
            sub_agent_values_repo,
            file_reader_mock,
            true,
        );

        let result = assembler.assemble_agent(
            &AgentID::new("first").unwrap(),
            &agent_config
                .agents
                .get(&AgentID::new("first").unwrap())
                .unwrap(),
        );

        assert!(result.is_err());
        assert_eq!(
            "error assembling agents: `file error: `error creating file: `permission denied```"
                .to_string(),
            result.err().unwrap().to_string()
        );
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
        let first_agent_id = AgentID::new("first").unwrap();
        let second_agent_id = AgentID::new("second").unwrap();
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
                assert!(res.is_ok());
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

                assert!(res.is_ok());
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
                    },
                ),
            ])
            .into(),
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
