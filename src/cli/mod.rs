use std::path::PathBuf;

use clap::Parser;

use crate::config::{
    agent_configs::MetaAgentConfig, error::MetaAgentConfigError, resolver::Resolver,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)] // Read from `Cargo.toml`
struct MetaAgentCli {
    #[arg(short, long, default_value_t = String::from("/tmp/static.yml"))]
    config: String,
}

/// Parses command line arguments and retrieves the passed configuration
pub fn init_meta_agent() -> Result<MetaAgentConfig, MetaAgentConfigError> {
    let cli = init_meta_agent_cli();
    Resolver::retrieve_config(&PathBuf::from(cli.config))
}

fn init_meta_agent_cli() -> MetaAgentCli {
    // Get command line args
    MetaAgentCli::parse()
}
