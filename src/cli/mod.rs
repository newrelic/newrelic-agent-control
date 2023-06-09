use std::path::PathBuf;

use clap::Parser;

use crate::config::{
    agent_configs::MetaAgentConfig, error::MetaAgentConfigError, resolver::Resolver,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
struct MetaAgentCli {
    #[arg(short, long)]
    config: Option<PathBuf>,
}

/// Parses command line arguments and retrieves the passed configuration
pub fn init_meta_agent() -> Result<MetaAgentConfig, MetaAgentConfigError> {
    let cli = init_meta_agent_cli();
    Resolver::retrieve_config(cli.config.as_deref())
}

fn init_meta_agent_cli() -> MetaAgentCli {
    // Get command line args
    MetaAgentCli::parse()
}
