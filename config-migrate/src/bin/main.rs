use config_migrate::cli::Cli;
use config_migrate::migration::agent_config_getter::AgentConfigGetter;
use config_migrate::migration::config::MigrationConfig;
use config_migrate::migration::converter::ConfigConverter;
use config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
use config_migrate::migration::migrator::ConfigMigrator;
use config_migrate::migration::persister::legacy_config_renamer::LegacyConfigRenamer;
use config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use log::info;
use newrelic_super_agent::config::store::SuperAgentConfigStoreFile;
use newrelic_super_agent::logging::Logging;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    info!("Starting conversion tool...");

    let config: MigrationConfig = serde_yaml::from_str(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING)?;

    let cli = Cli::init_config_migrate_cli();
    let local_config_path = cli.get_config_path();
    let super_agent_config_loader =
        SuperAgentConfigStoreFile::new(&local_config_path).with_remote()?;
    let config_migrator = ConfigMigrator::new(
        ConfigConverter::default(),
        AgentConfigGetter::new(super_agent_config_loader),
        ValuesPersisterFile::default(),
    );

    let legacy_config_renamer = LegacyConfigRenamer::default();

    for cfg in config.configs {
        config_migrator.migrate(&cfg)?;

        for (_, dir_path) in cfg.dirs_map {
            legacy_config_renamer.rename_path(dir_path.as_str())?;
        }
        for (_, file_path) in cfg.files_map {
            legacy_config_renamer.rename_path(file_path.as_str())?;
        }
        info!("Old config files and paths renamed");
    }
    info!("Config files successfully converted");

    Ok(())
}
