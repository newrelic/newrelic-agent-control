use newrelic_agent_control::agent_control::config_storer::store::AgentControlConfigStore;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR, SUB_AGENT_DIR,
};
use newrelic_agent_control::config_migrate::cli::Cli;
use newrelic_agent_control::config_migrate::migration::agent_config_getter::AgentConfigGetter;
use newrelic_agent_control::config_migrate::migration::config::MigrationConfig;
use newrelic_agent_control::config_migrate::migration::converter::ConfigConverter;
use newrelic_agent_control::config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING;
use newrelic_agent_control::config_migrate::migration::migrator::{ConfigMigrator, MigratorError};
use newrelic_agent_control::config_migrate::migration::persister::legacy_config_renamer::LegacyConfigRenamer;
use newrelic_agent_control::config_migrate::migration::persister::values_persister_file::ValuesPersisterFile;
use newrelic_agent_control::instrumentation::tracing::{try_init_tracing, TracingConfig};
use newrelic_agent_control::values::file::YAMLConfigRepositoryFile;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

fn main() -> Result<(), Box<dyn Error>> {
    let tracing_config = TracingConfig::from_logging_path(PathBuf::from(AGENT_CONTROL_LOG_DIR));
    let _tracer = try_init_tracing(tracing_config);

    info!("Starting config conversion tool...");

    let config: MigrationConfig = MigrationConfig::parse(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING)?;

    let cli = Cli::init_config_migrate_cli();
    let remote_dir = PathBuf::from(AGENT_CONTROL_DATA_DIR);
    let vr = YAMLConfigRepositoryFile::new(cli.local_data_dir(), remote_dir);
    let sa_local_config_loader = AgentControlConfigStore::new(Arc::new(vr));
    let config_migrator = ConfigMigrator::new(
        ConfigConverter::default(),
        AgentConfigGetter::new(sa_local_config_loader),
        ValuesPersisterFile::new(PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR).join(SUB_AGENT_DIR)),
    );

    let legacy_config_renamer = LegacyConfigRenamer::default();

    for cfg in config.configs {
        debug!("Checking configurations for {}", cfg.agent_type_fqn);
        match config_migrator.migrate(&cfg) {
            Ok(_) => {
                for (_, dir_path) in cfg.dirs_map {
                    legacy_config_renamer.rename_path(dir_path.path.as_path())?;
                }
                for (_, file_path) in cfg.files_map {
                    legacy_config_renamer.rename_path(file_path.as_path())?;
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
