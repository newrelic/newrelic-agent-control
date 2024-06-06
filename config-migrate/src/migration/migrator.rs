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
use newrelic_super_agent::agent_type::agent_type_registry::AgentRegistry;
use newrelic_super_agent::agent_type::embedded_registry::EmbeddedRegistry;
use newrelic_super_agent::super_agent::config::SuperAgentConfigError;
use newrelic_super_agent::super_agent::config_storer::loader_storer::SuperAgentDynamicConfigLoader;
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
use thiserror::Error;
use tracing::{debug, error, info};

#[derive(Error, Debug)]
pub enum MigratorError {
    #[error("")]
    AgentTypeNotFoundOnConfig,

    #[error("`{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),

    #[error("configuration is not valid YAML: `{0}`")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),

    #[error("error persisting values file: `{0}`")]
    PersistError(#[from] PersistError),

    #[error("`{0}`")]
    ConversionError(#[from] ConversionError),
}

pub struct ConfigMigrator<
    R: AgentRegistry,
    SL: SuperAgentDynamicConfigLoader + 'static,
    C: DirectoryManager,
    F: FileReader,
> {
    config_converter: ConfigConverter<R, F>,
    agent_config_getter: AgentConfigGetter<SL>,
    values_persister: ValuesPersisterFile<C>,
}

impl ConfigMigrator<EmbeddedRegistry, SuperAgentConfigStore, DirectoryManagerFs, LocalFile> {
    pub fn new(
        config_converter: ConfigConverter<EmbeddedRegistry, LocalFile>,
        agent_config_getter: AgentConfigGetter<SuperAgentConfigStore>,
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
            .get_agents_of_type_between_versions(cfg.agent_type_fqn.clone(), cfg.next.clone())
        else {
            return Err(MigratorError::AgentTypeNotFoundOnConfig);
        };

        for (agent_id, _) in sub_agents_cfg.agents {
            debug!("preparing to migrate agent_id: {}", agent_id);
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
    use newrelic_super_agent::super_agent::config::{
        AgentID, AgentTypeFQN, SubAgentConfig, SuperAgentDynamicConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn test_migrate() {
        let agent_a = AgentID::new("infra-agent-a").unwrap();
        let agent_b = AgentID::new("infra-agent-b").unwrap();
        let agents: HashMap<AgentID, SubAgentConfig> = HashMap::from([
            (
                agent_a.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            ),
            (
                agent_b.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            ),
        ]);

        let mut agent_config_getter = MockAgentConfigGetter::default();
        agent_config_getter
            .expect_get_agents_of_type_between_versions()
            .once()
            .returning(move |_, _| {
                Ok(SuperAgentDynamicConfig {
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
            agent_type_fqn: AgentTypeFQN::try_from(
                "newrelic/com.newrelic.infrastructure_agent:0.0.1",
            )
            .unwrap(),
            files_map: Default::default(),
            dirs_map: Default::default(),
            next: None,
        };
        let migration = migrator.migrate(&agent_config_mapping);
        assert!(migration.is_ok());
    }
}
