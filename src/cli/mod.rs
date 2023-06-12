use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(short, long, default_value_t = String::from("/tmp/static.yml"))]
    config: String,

    #[arg(long)]
    print_debug_info: bool,
}

impl Cli {
    /// Parses command line arguments
    pub fn init_meta_agent_cli() -> Self {
        // Get command line args
        Self::parse()
    }

    pub fn get_config(&self) -> PathBuf {
        PathBuf::from(&self.config)
    }

    pub fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
