use log::{error, info};
use newrelic_super_agent::config::store::{SuperAgentConfigLoader, SuperAgentConfigStoreFile};
use newrelic_super_agent::config::super_agent_configs::{AgentID, AgentTypeFQN};
use newrelic_super_agent::config_migrate::config::MigrationConfig;
use newrelic_super_agent::config_migrate::converter::ConfigConverter;
use newrelic_super_agent::config_migrate::defaults::{
    DEFAULT_AGENT_ID, NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING,
};
use newrelic_super_agent::config_migrate::legacy_config_renamer::LegacyConfigRenamer;
use newrelic_super_agent::config_migrate::values_persister_file::ValuesPersisterFile;
use newrelic_super_agent::logging::Logging;
use std::error::Error;
use std::path::PathBuf;
const DEFAULT_CFG_PATH: &str = "/etc/newrelic-super-agent/config.yaml";

fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    info!("Starting conversion tool...");

    let config: MigrationConfig = serde_yaml::from_str(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING)?;

    let config_converter = ConfigConverter::default();
    let values_persister = ValuesPersisterFile::default();
    let legacy_config_renamer = LegacyConfigRenamer::default();

    for cfg in config.configs {
        let Ok(agent_type) = get_current_agent_type().map_err(|e| {
            error!("Error finding newrelic-super-agent config: {}", e);
        }) else {
            return Ok(());
        };

        if cfg.agent_type_fqn != agent_type {
            error!("This script requires a config file and nr_infra_agent agent_type 0.0.2");
            return Ok(());
        }

        match config_converter.convert(&cfg) {
            Ok(agent_variables) => {
                let values_content = serde_yaml::to_string(&agent_variables)?;
                values_persister.persist_values_file(
                    &AgentID::new(DEFAULT_AGENT_ID)?,
                    values_content.as_str(),
                )?;
                for (_, dir_path) in cfg.dirs_map {
                    legacy_config_renamer.rename_path(dir_path.as_str())?;
                }
                for (_, file_path) in cfg.files_map {
                    legacy_config_renamer.rename_path(file_path.as_str())?;
                }
                info!("Config files successfully converted");
            }
            Err(e) => {
                error!(
                    "Conversion failed, old files or paths are renamed or not present: {}",
                    e
                );
            }
        };
    }

    Ok(())
}

// TODO : Add tests for getting this agent_type and check the agent_type by namespace instead of name DEFAULT_AGENT_ID
fn get_current_agent_type() -> Result<AgentTypeFQN, Box<dyn Error>> {
    let local_config_path = PathBuf::from(DEFAULT_CFG_PATH.to_string());
    let super_agent_config_storer =
        SuperAgentConfigStoreFile::new(&local_config_path).with_remote()?;
    let agents = super_agent_config_storer.load()?.agents;
    Ok(agents
        .get(&AgentID::new(DEFAULT_AGENT_ID)?)
        .cloned()?
        .agent_type)
}
