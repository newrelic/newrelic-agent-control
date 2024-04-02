use config_migrate::cli::Cli;
use config_migrate::migration::agent_config_getter::AgentConfigGetter;
use config_migrate::migration::config::MigrationConfig;
use config_migrate::migration::converter::ConfigConverter;
use config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
use config_migrate::migration::migrator::{ConfigMigrator, MigratorError};
use config_migrate::migration::persister::legacy_config_renamer::LegacyConfigRenamer;
use config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use newrelic_super_agent::logging::config::LoggingConfig;
use newrelic_super_agent::super_agent::config_storer::SuperAgentConfigStoreFile;
use std::error::Error;
use tracing::{debug, info};

fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    LoggingConfig::default().try_init()?;

    info!("Starting conversion tool...");

    let config: MigrationConfig = serde_yaml::from_str(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING)?;

    let cli = Cli::init_config_migrate_cli();
    let local_config_path = cli.get_config_path();
    let sa_local_config_loader = SuperAgentConfigStoreFile::new(&local_config_path);
    let config_migrator = ConfigMigrator::new(
        ConfigConverter::default(),
        AgentConfigGetter::new(sa_local_config_loader),
        ValuesPersisterFile::default(),
    );

    let legacy_config_renamer = LegacyConfigRenamer::default();

    for cfg in config.configs {
        debug!("Checking configurations for {}", cfg.agent_type_fqn);
        match config_migrator.migrate(&cfg) {
            Ok(_) => {
                for (_, dir_path) in cfg.dirs_map {
                    legacy_config_renamer.rename_path(dir_path.as_str())?;
                }
                for (_, file_path) in cfg.files_map {
                    legacy_config_renamer.rename_path(file_path.as_str())?;
                }
                info!("Old config files and paths renamed");
            }
            Err(MigratorError::AgentTypeNotFoundOnConfig) => {
                debug!(
                    "No agents of agent_type {} found on config, skipping",
                    cfg.agent_type_fqn.clone()
                );
            }
            Err(e) => {
                return Err(Box::new(e));
            }
        }
    }
    info!("Config files successfully converted");

    Ok(())
}
