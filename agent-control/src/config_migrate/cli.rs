use crate::{
    agent_control::defaults::{
        AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    },
    config_migrate::migration::defaults::NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING,
};
use clap::Parser;
use std::{error::Error, fs, path::PathBuf};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    /// Local data path used by Agent Control.
    #[arg(long, default_value_os_t = PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR))]
    local_dir: PathBuf,

    /// Remote data path used by Agent Control.
    #[arg(long, default_value_os_t = PathBuf::from(AGENT_CONTROL_DATA_DIR))]
    remote_dir: PathBuf,

    /// Logs path used by Agent Control.
    #[arg(long, default_value_os_t = PathBuf::from(AGENT_CONTROL_LOG_DIR))]
    logs_dir: PathBuf,

    /// Provides an external configuration mapping for the migration of agents to Agent Control.
    #[arg(long)]
    migration_config_file: Option<PathBuf>,
}

impl Cli {
    /// Parses command line arguments
    pub fn load() -> Self {
        // Get command line args
        Self::parse()
    }

    pub fn local_data_dir(&self) -> PathBuf {
        self.local_dir.to_path_buf()
    }

    pub fn remote_data_dir(&self) -> PathBuf {
        self.remote_dir.to_path_buf()
    }

    pub fn log_dir(&self) -> PathBuf {
        self.logs_dir.to_path_buf()
    }

    pub fn get_migration_config_str(&self) -> Result<String, Box<dyn Error>> {
        if let Some(path) = &self.migration_config_file {
            fs::read_to_string(path).map_err(|e| {
                format!(
                    "Could not read provided migration config file ({}): {}",
                    path.display(),
                    e
                )
                .into()
            })
        } else {
            Ok(NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING.to_owned())
        }
    }
}
