use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};
use newrelic_agent_control::cli::common::{agent_type_validation, error::CliError, logs};
use newrelic_agent_control::cli::k8s::install::agent_control::InstallAgentControl;
use newrelic_agent_control::cli::k8s::install::flux::InstallFlux;
use newrelic_agent_control::cli::k8s::install::{InstallData, apply_resources};
use newrelic_agent_control::cli::k8s::system_identity::{self, register_system_identity};
use newrelic_agent_control::cli::k8s::uninstall::agent_control::{
    AgentControlUninstallData, uninstall_agent_control,
};
use newrelic_agent_control::cli::k8s::uninstall::flux::{FluxUninstallData, remove_flux_crs};
use std::process::ExitCode;
use tracing::{Level, error};

/// Manage agent control resources
#[derive(Debug, Parser)]
#[command()]
struct Cli {
    #[command(subcommand)]
    operation: Operations,

    /// Namespace where resources of agent control are created
    #[arg(short, long, global = true, default_value = "default")]
    namespace: String,

    /// Log level upperbound
    #[arg(long, global = true, default_value = "info")]
    log_level: Level,
}

#[derive(Debug, Subcommand)]
enum Operations {
    /// Install agent control chart and create required resources
    InstallAgentControl(InstallData),

    /// Uninstall agent control and delete related resources
    UninstallAgentControl(AgentControlUninstallData),

    /// Create the resources needed to handle the Continuous Deployment utility (currently Flux) from Agent Control
    #[clap(name = "create-cd-resources")]
    CreateCDResources(InstallData),

    /// Remove the resources created to handled the Continuous Deployment utility
    #[clap(name = "remove-cd-resources")]
    RemoveCDResources(FluxUninstallData),

    /// Registers the System Identity to be used in Agent Control for authentication.
    RegisterSystemIdentity(system_identity::Args),

    /// Operations on agent type definitions.
    #[command(subcommand)]
    AgentType(AgentTypeCommand),
}

/// Commands to operate on agent type definitions.
#[derive(Debug, Subcommand)]
enum AgentTypeCommand {
    /// Validates an agent type definition file (schema-level checks only).
    Validate(agent_type_validation::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let tracer = logs::init(cli.log_level);
    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {err}");
        return err.into();
    }

    let result = match cli.operation {
        Operations::InstallAgentControl(agent_control_data) => {
            apply_resources(InstallAgentControl, &cli.namespace, &agent_control_data)
                .map_err(CliError::from)
        }
        Operations::UninstallAgentControl(agent_control_data) => {
            uninstall_agent_control(&cli.namespace, &agent_control_data).map_err(CliError::from)
        }
        Operations::CreateCDResources(cd_data) => {
            // Currently this means installing Flux, but in the future it could mean other CD tool
            // or support different ones
            apply_resources(InstallFlux, &cli.namespace, &cd_data).map_err(CliError::from)
        }
        Operations::RemoveCDResources(cd_data) => {
            remove_flux_crs(&cli.namespace, &cd_data.release_name).map_err(CliError::from)
        }
        Operations::RegisterSystemIdentity(args) => match args.validate() {
            Ok(spec) => register_system_identity(&cli.namespace, spec).map_err(CliError::from),
            Err(err) => {
                let mut cmd = Cli::command();
                cmd.error(ErrorKind::ArgumentConflict, err.to_string())
                    .exit()
            }
        },
        Operations::AgentType(AgentTypeCommand::Validate(args)) => {
            agent_type_validation::validate(args)
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Operation failed: {}", err);
            err.into()
        }
    }
}
