#[cfg_attr(test, mockall_double::double)]
use crate::migration::agent_config_getter::AgentConfigGetter;
use crate::migration::config::MigrationAgentConfig;
#[cfg_attr(test, mockall_double::double)]
use crate::migration::converter::ConfigConverter;
use crate::migration::converter::ConversionError;
use crate::migration::persister::values_persister_file::PersistError;
#[cfg_attr(test, mockall_double::double)]
use crate::migration::persister::values_persister_file::ValuesPersisterFile;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use fs::file_reader::FileReader;
use fs::LocalFile;
use log::{error, info};
use newrelic_super_agent::config::agent_type_registry::{AgentRegistry, LocalRegistry};
use newrelic_super_agent::config::error::SuperAgentConfigError;
use newrelic_super_agent::config::store::{SubAgentsConfigLoader, SuperAgentConfigStoreFile};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MigratorError {
    #[error("`{0}`")]
    ConversionError(#[from] ConversionError),

    #[error("`{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),

    #[error("configuration is not valid YAML: `{0}`")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),

    #[error("error persisting values file: `{0}`")]
    PersistError(#[from] PersistError),
}

pub struct ConfigMigrator<
    R: AgentRegistry,
    SL: SubAgentsConfigLoader + 'static,
    C: DirectoryManager,
    F: FileReader,
> {
    config_converter: ConfigConverter<R, F>,
    agent_config_getter: AgentConfigGetter<SL>,
    values_persister: ValuesPersisterFile<C>,
}

impl ConfigMigrator<LocalRegistry, SuperAgentConfigStoreFile, DirectoryManagerFs, LocalFile> {
    pub fn new(
        config_converter: ConfigConverter<LocalRegistry, LocalFile>,
        agent_config_getter: AgentConfigGetter<SuperAgentConfigStoreFile>,
        values_persister: ValuesPersisterFile<DirectoryManagerFs>,
    ) -> Self {
        ConfigMigrator {
            config_converter,
            agent_config_getter,
            values_persister,
        }
    }

    pub fn migrate(&self, cfg: &MigrationAgentConfig) -> Result<(), MigratorError> {
        let Ok(sub_agents_cfg) = self
            .agent_config_getter
            .get_agents_of_type(cfg.agent_type_fqn.clone())
            .map_err(|e| {
                error!("Error finding newrelic-super-agent config");
                e
            })
        else {
            return Ok(());
        };

        for (agent_id, _) in sub_agents_cfg.agents {
            match self.config_converter.convert(cfg) {
                Ok(agent_variables) => {
                    let values_content = serde_yaml::to_string(&agent_variables)?;
                    self.values_persister
                        .persist_values_file(&agent_id, values_content.as_str())?;
                    info!("Config values files successfully created for {}", agent_id);
                }
                Err(e) => {
                    error!("Conversion failed, old files or paths are renamed or not present");
                    return Err(MigratorError::ConversionError(e));
                }
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::migration::agent_config_getter::MockAgentConfigGetter;
    use crate::migration::agent_value_spec::AgentValueSpec::AgentValueSpecEnd;
    use crate::migration::config::MigrationAgentConfig;
    use crate::migration::converter::MockConfigConverter;
    use crate::migration::migrator::ConfigMigrator;
    use crate::migration::persister::values_persister_file::MockValuesPersisterFile;
    use mockall::predicate;
    use newrelic_super_agent::config::super_agent_configs::{
        AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn test_migrate() {
        let agent_a = AgentID::new("infra_agent_a").unwrap();
        let agent_b = AgentID::new("infra_agent_b").unwrap();
        let agents: HashMap<AgentID, SubAgentConfig> = HashMap::from([
            (
                agent_a.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
                },
            ),
            (
                agent_b.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
                },
            ),
        ])
        .into();

        let mut agent_config_getter = MockAgentConfigGetter::default();
        agent_config_getter
            .expect_get_agents_of_type()
            .once()
            .returning(move |_| {
                Ok(SubAgentsConfig {
                    agents: agents.clone(),
                })
            });

        let agent_variables =
            HashMap::from([("cfg".to_string(), AgentValueSpecEnd("value".to_string()))]);

        let mut config_converter = MockConfigConverter::default();
        config_converter
            .expect_convert()
            .times(2)
            .returning(move |_| Ok(agent_variables.clone()));

        let mut values_persister = MockValuesPersisterFile::default();
        values_persister
            .expect_persist_values_file()
            .with(predicate::eq(agent_a), predicate::always())
            .once()
            .returning(|_, _| Ok(()));
        values_persister
            .expect_persist_values_file()
            .with(predicate::eq(agent_b), predicate::always())
            .once()
            .returning(|_, _| Ok(()));

        let migrator = ConfigMigrator::new(config_converter, agent_config_getter, values_persister);

        let agent_config_mapping = MigrationAgentConfig {
            agent_type_fqn: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
            files_map: Default::default(),
            dirs_map: Default::default(),
        };
        let migration = migrator.migrate(&agent_config_mapping);
        assert!(migration.is_ok());
    }
}
