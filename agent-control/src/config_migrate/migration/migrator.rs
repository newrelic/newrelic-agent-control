use crate::agent_control::config::AgentControlConfigError;
use crate::agent_control::config_repository::repository::AgentControlDynamicConfigRepository;
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_type::agent_type_registry::AgentRegistry;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
#[cfg_attr(test, mockall_double::double)]
use crate::config_migrate::migration::agent_config_getter::AgentConfigGetter;
use crate::config_migrate::migration::config::MigrationAgentConfig;
#[cfg_attr(test, mockall_double::double)]
use crate::config_migrate::migration::converter::ConfigConverter;
use crate::config_migrate::migration::converter::ConversionError;
use crate::config_migrate::migration::persister::values_persister_file::PersistError;
#[cfg_attr(test, mockall_double::double)]
use crate::config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use crate::values::file::ConfigRepositoryFile;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use fs::file_reader::FileReader;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum MigratorError {
    #[error("")]
    AgentTypeNotFoundOnConfig,

    #[error("{0}")]
    AgentControlConfigError(#[from] AgentControlConfigError),

    #[error("configuration is not valid YAML: {0}")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),

    #[error("error persisting values file: {0}")]
    PersistError(#[from] PersistError),

    #[error("{0}")]
    ConversionError(#[from] ConversionError),
}

pub struct ConfigMigrator<
    R: AgentRegistry,
    SL: AgentControlDynamicConfigRepository + 'static,
    C: DirectoryManager,
    F: FileReader,
> {
    config_converter: ConfigConverter<R, F>,
    agent_config_getter: AgentConfigGetter<SL>,
    values_persister: ValuesPersisterFile<C>,
}

impl
    ConfigMigrator<
        EmbeddedRegistry,
        AgentControlConfigStore<ConfigRepositoryFile<LocalFile, DirectoryManagerFs>>,
        DirectoryManagerFs,
        LocalFile,
    >
{
    pub fn new(
        config_converter: ConfigConverter<EmbeddedRegistry, LocalFile>,
        agent_config_getter: AgentConfigGetter<
            AgentControlConfigStore<ConfigRepositoryFile<LocalFile, DirectoryManagerFs>>,
        >,
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
            debug!(
                "preparing to migrate local config for agent_id: {}",
                agent_id
            );
            match self.config_converter.convert(cfg) {
                Ok(agent_variables) => {
                    let values_content = serde_yaml::to_string(&agent_variables)?;
                    self.values_persister
                        .persist_values_file(&agent_id, values_content.as_str())?;
                    info!(
                        "Local config values files successfully created for {}",
                        agent_id
                    );
                }
                Err(e) => {
                    warn!("Old files or paths are renamed or not present");
                    return Err(MigratorError::ConversionError(e));
                }
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::{AgentControlDynamicConfig, SubAgentConfig};
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::config_migrate::migration::agent_config_getter::MockAgentConfigGetter;
    use crate::config_migrate::migration::agent_value_spec::AgentValueSpec::AgentValueSpecEnd;
    use crate::config_migrate::migration::config::MigrationAgentConfig;
    use crate::config_migrate::migration::converter::MockConfigConverter;
    use crate::config_migrate::migration::migrator::ConfigMigrator;
    use crate::config_migrate::migration::persister::values_persister_file::MockValuesPersisterFile;
    use mockall::predicate;
    use std::collections::HashMap;

    #[test]
    fn test_migrate() {
        let agent_a = AgentID::try_from("infra-agent-a").unwrap();
        let agent_b = AgentID::try_from("infra-agent-b").unwrap();
        let agents: HashMap<AgentID, SubAgentConfig> = HashMap::from([
            (
                agent_a.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                        .unwrap(),
                },
            ),
            (
                agent_b.clone(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                        .unwrap(),
                },
            ),
        ]);

        let mut agent_config_getter = MockAgentConfigGetter::default();
        agent_config_getter
            .expect_get_agents_of_type_between_versions()
            .once()
            .returning(move |_, _| {
                Ok(AgentControlDynamicConfig {
                    agents: agents.clone(),
                    chart_version: None,
                    cd_chart_version: None,
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
            agent_type_fqn: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1")
                .unwrap(),
            files_map: Default::default(),
            dirs_map: Default::default(),
            next: None,
        };
        let migration = migrator.migrate(&agent_config_mapping);
        assert!(migration.is_ok());
    }
}
