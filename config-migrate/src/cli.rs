use clap::Parser;
use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_LOCAL_DATA_DIR;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    /// Overrides the default local configuration path `/etc/newrelic-super-agent/`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    local_dir: Option<PathBuf>,
}

impl Cli {
    /// Parses command line arguments
    pub fn init_config_migrate_cli() -> Self {
        // Get command line args
        Self::parse()
    }

    pub fn local_data_dir(&self) -> PathBuf {
        if let Some(path) = &self.local_dir {
            return path.clone();
        }
        PathBuf::from(SUPER_AGENT_LOCAL_DATA_DIR)
    }
}
