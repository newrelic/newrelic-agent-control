use crate::agent_control::config::AgentControlConfigError;
use crate::agent_control::config_repository::repository::AgentControlDynamicConfigRepository;
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::config_migrate::migration::agent_config_getter::AgentConfigGetter;
use crate::config_migrate::migration::config::MigrationAgentConfig;
use crate::config_migrate::migration::converter::ConfigConverter;
use crate::config_migrate::migration::converter::ConversionError;
use crate::config_migrate::migration::persister::values_persister_file::PersistError;
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
    SL: AgentControlDynamicConfigRepository + 'static,
    C: DirectoryManager,
    F: FileReader,
> {
    config_converter: ConfigConverter<F>,
    agent_config_getter: AgentConfigGetter<SL>,
    values_persister: ValuesPersisterFile<C>,
}

impl
    ConfigMigrator<
        AgentControlConfigStore<ConfigRepositoryFile<LocalFile, DirectoryManagerFs>>,
        DirectoryManagerFs,
        LocalFile,
    >
{
    pub fn new(
        config_converter: ConfigConverter<LocalFile>,
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
    use crate::agent_control::config_repository::store::AgentControlConfigStore;
    use crate::agent_control::defaults::SUB_AGENT_DIR;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::config_migrate::migration::agent_config_getter::AgentConfigGetter;
    use crate::config_migrate::migration::config::MigrationAgentConfig;
    use crate::config_migrate::migration::converter::ConfigConverter;
    use crate::config_migrate::migration::migrator::ConfigMigrator;
    use crate::config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
    use crate::values::file::ConfigRepositoryFile;
    use std::sync::Arc;
    use tempfile::TempDir;

    const INITIAL_INFRA_CONFIG: &str = r#"
license_key: invented
enable_process_metrics: false
status_server_enabled: false
status_server_port: 2333
extra_config: true
"#;

    const AGENTS_CONFIG: &str = r#"
agents:
    infra-agent-a:
        agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
    infra-agent-b:
        agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
"#;

    #[cfg(target_family = "unix")] //TODO This should be removed when Windows support is added
    #[test]
    fn test_migrate() {
        let tmp_dir = TempDir::new().unwrap();
        let infra_file_path = tmp_dir.path().join("newrelic-infra.yml");
        let agents_file_path = tmp_dir.path().join("config.yaml");

        // Emulate the existence of the file by creating it
        std::fs::write(&infra_file_path, INITIAL_INFRA_CONFIG).unwrap();

        std::fs::write(&agents_file_path, AGENTS_CONFIG).unwrap();

        let vr =
            ConfigRepositoryFile::new(tmp_dir.path().to_path_buf(), tmp_dir.path().to_path_buf());
        let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(vr));

        let config_migrator = ConfigMigrator::new(
            ConfigConverter::default(),
            AgentConfigGetter::new(sa_local_config_loader),
            ValuesPersisterFile::new(tmp_dir.path().join(SUB_AGENT_DIR)),
        );

        let agent_config_mapping = MigrationAgentConfig {
            agent_type_fqn: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1")
                .unwrap(),
            filesystem_mappings: Default::default(),
            next: None,
        };
        let migration = config_migrator.migrate(&agent_config_mapping);
        assert!(migration.is_ok());

        let values_file = tmp_dir
            .path()
            .join("fleet/agents.d/infra-agent-a/values/values.yaml");
        assert!(std::fs::exists(&values_file).unwrap());

        let values_file = tmp_dir
            .path()
            .join("fleet/agents.d/infra-agent-b/values/values.yaml");
        assert!(std::fs::exists(&values_file).unwrap());
    }
}
