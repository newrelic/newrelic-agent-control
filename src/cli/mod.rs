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

pub fn init_meta_agent() -> Result<MetaAgentConfig, Box<dyn std::error::Error>> {
    let cli = init_meta_agent_cli();
    Ok(retrieve_config(cli)?)
}

fn init_meta_agent_cli() -> MetaAgentCli {
    // Get command line args
    MetaAgentCli::parse()
}

fn retrieve_config(cli: MetaAgentCli) -> Result<MetaAgentConfig, MetaAgentConfigError> {
    // Program starts, should load the config
    let cfg = if let Some(file) = cli.config.as_deref() {
        Resolver::new(file).build_config()?
    } else {
        Resolver::default().build_config()?
    };
    Ok(cfg)
}
