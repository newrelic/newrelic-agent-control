//! Implementation of the generate-config command for the on-host cli.

use crate::cli::error::CliError;
use crate::cli::on_host::config_gen::config::AgentSet;
use crate::cli::on_host::config_gen::region::{Region, region_parser};
use crate::cli::on_host::host_monitoring_gen::infra_config_gen::InfraConfigGenerator;
use crate::cli::on_host::host_monitoring_gen::otel_config_gen::OtelConfigGen;
use tracing::info;

pub mod infra_config;
pub mod infra_config_gen;
pub mod otel_config_gen;

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
    #[arg(long, value_parser = region_parser(), required = true)]
    region: Region,
}

/// Generates the Host monitoring values either infra-agent or otel.
pub fn generate_host_monitoring_config(args: Args) -> Result<(), CliError> {
    info!("Generating Host monitoring values");

    match args.agent_set {
        AgentSet::InfraAgent => {
            let infra_config_generator = InfraConfigGenerator::default();

            infra_config_generator
                .generate_infra_config(args.region, args.custom_attributes, args.proxy)
                .map_err(|err| {
                    CliError::Command(format!("failed generating infra config: {err}"))
                })?;
        }
        AgentSet::Otel => {
            let otel_config_generator = OtelConfigGen::default();
            otel_config_generator.generate_otel_config()?;
        }
    }

    info!("Host monitoring values generated successfully");
    Ok(())
}
