use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
pub struct MetaAgentCli {
    #[arg(short, long)]
    config: Option<PathBuf>,
}

impl MetaAgentCli {
    /// Gets the config passed as argument to the program, if any
    pub fn get_config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// Parses the command line arguments
    pub fn init() -> Self {
        Self::parse()
    }
}
