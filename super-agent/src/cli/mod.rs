use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(short, long, default_value_t = String::from("/etc/newrelic-super-agent/config.yaml"))]
    config: String,

    #[arg(long)]
    print_debug_info: bool,

    #[arg(long)]
    version: bool,

    #[arg(long)]
    pub local_dir: Option<PathBuf>,

    #[arg(long)]
    pub remote_dir: Option<PathBuf>,

    #[arg(long)]
    pub logs_dir: Option<PathBuf>,

    #[arg(long, value_name = "DATA_DIR")]
    pub debug: Option<PathBuf>,
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

    pub fn print_version(&self) -> bool {
        self.version
    }

    pub fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
