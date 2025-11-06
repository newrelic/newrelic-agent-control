use newrelic_agent_control::agent_control::config_repository::store::AgentControlConfigStore;
use newrelic_agent_control::config_migrate::cli::Cli;
use newrelic_agent_control::config_migrate::migration::agent_config_getter::AgentConfigGetter;
use newrelic_agent_control::config_migrate::migration::config::{MappingType, MigrationConfig};
use newrelic_agent_control::config_migrate::migration::converter::ConfigConverter;
use newrelic_agent_control::config_migrate::migration::migrator::{ConfigMigrator, MigratorError};
use newrelic_agent_control::config_migrate::migration::persister::legacy_config_renamer::LegacyConfigRenamer;
use newrelic_agent_control::config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use newrelic_agent_control::instrumentation::tracing::{TracingConfig, try_init_tracing};
use newrelic_agent_control::on_host::file_store::FileStore;
use newrelic_agent_control::values::ConfigRepo;
use std::error::Error;
use std::sync::Arc;
use tracing::{debug, info, warn};

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::load();
    let tracing_config = TracingConfig::from_logging_path(cli.log_dir());
    let _tracer = try_init_tracing(tracing_config)?;

    info!("Starting config conversion tool...");

    let config = MigrationConfig::parse(&cli.get_migration_config_str()?)?;

    let file_store = Arc::new(FileStore::new_local_fs(
        cli.local_data_dir(),
        cli.remote_data_dir(),
    ));
    let config_repository = ConfigRepo::new(file_store);
    let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(config_repository));
    let config_migrator = ConfigMigrator::new(
        ConfigConverter::default(),
        AgentConfigGetter::new(sa_local_config_loader),
        ValuesPersisterFile::new(cli.local_data_dir()),
    );

    let legacy_config_renamer = LegacyConfigRenamer::default();

    for cfg in config.configs {
        debug!("Checking configurations for {}", cfg.agent_type_fqn);
        match config_migrator.migrate(&cfg) {
            Ok(_) => {
                for (_, mapping_type) in cfg.filesystem_mappings {
                    match mapping_type {
                        MappingType::Dir(dir_path) => {
                            legacy_config_renamer.rename_path(dir_path.dir_path.as_path())?
                        }
                        MappingType::File(file_info) => {
                            legacy_config_renamer.rename_path(file_info.file_path.as_path())?
                        }
                    }
                }
                debug!("Classic config files and paths renamed");
            }
            Err(MigratorError::AgentTypeNotFoundOnConfig) => {
                debug!(
                    "No agents of agent_type {} found on config, skipping",
                    cfg.agent_type_fqn.clone()
                );
            }
            Err(e) => {
                warn!(
                    "Could not apply local config migration for {}: {}",
                    cfg.agent_type_fqn, e
                );
            }
        }
    }
    info!("Local config files successfully created");

    Ok(())
}
