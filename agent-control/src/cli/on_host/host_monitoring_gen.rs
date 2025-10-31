//! Implementation of the generate-config command for the on-host cli.

use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::config::AgentSet;
use crate::cli::on_host::config_gen::region::{Region, region_parser};
use crate::cli::on_host::host_monitoring_gen::infra_config_gen::InfraConfigGenerator;
use tracing::info;

pub mod infra_config;
pub mod infra_config_gen;

/// Generates the Agent Control configuration for host environments.
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Agent to be used for host monitoring.
    #[arg(long, required = true)]
    agent_set: AgentSet,

    /// Custom Attributes
    #[arg(long)]
    custom_attributes: Option<String>,

    /// Proxy configuration
    #[arg(long)]
    proxy: Option<String>,

    /// New Relic region
    #[arg(long, value_parser = region_parser())]
    region: Region,
}

/// Generates the Host monitoring values either infra-agent or otel.
pub fn generate_host_monitoring_config(args: Args) -> Result<(), CliError> {
    info!("Generating Host monitoring values");

    if args.agent_set == AgentSet::InfraAgent {
        let infra_config_generator = InfraConfigGenerator::default();

        infra_config_generator
            .generate_infra_config(args.region, args.custom_attributes, args.proxy)
            .map_err(|err| CliError::Command(format!("failed generating infra config: {err}")))?;
    } else {
        // TODO: this is going to create otel config an a following PR
        println!(
            "Host monitoring source is not InfraAgent. Skipping infra configuration generation."
        );
    }

    info!("Host monitoring values generated successfully");
    Ok(())
}
