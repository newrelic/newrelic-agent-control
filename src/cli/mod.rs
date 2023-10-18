pub mod running_mode;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(short, long, default_value_t = String::from("/etc/newrelic-super-agent/config.yaml"))]
    config: String,

    #[arg(long)]
    print_debug_info: bool,

    #[arg(long, default_value_t = running_mode::AgentRunningMode::OnHost)]
    running_mode: running_mode::AgentRunningMode,
}

impl Cli {
    /// Parses command line arguments
    pub fn init_super_agent_cli() -> Self {
        // Get command line args
        Self::parse()
    }

    pub fn get_config_path(&self) -> PathBuf {
        PathBuf::from(&self.config)
    }

    pub fn get_running_mode(&self) -> running_mode::AgentRunningMode {
        self.running_mode
    }

    pub fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
