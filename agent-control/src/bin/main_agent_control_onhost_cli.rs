use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use newrelic_agent_control::cli::on_host::host_monitoring_gen;
use newrelic_agent_control::cli::{logs, on_host::config_gen};
use tracing::{Level, error};

#[derive(Debug, clap::Parser)]
#[command()]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Log level for the cli command
    #[arg(long, global = true, default_value = "info")]
    cli_log_level: Level,
}

/// Commands supported by the cli
#[allow(clippy::large_enum_variant)]
#[derive(Debug, clap::Subcommand)]
enum Commands {
    // Generate Agent Control configuration according to the provided configuration data.
    GenerateConfig(config_gen::Args),
    // Generate Host Monitoring configuration according to the provided configuration data.
    GenerateHostMonitoring(host_monitoring_gen::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let tracer = logs::init(cli.cli_log_level);
    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {err}");
        return err.into();
    }

    let result = match cli.command {
        Commands::GenerateConfig(args) => {
            if let Err(err) = args.validate() {
                let mut cmd = Cli::command();
                cmd.error(ErrorKind::ArgumentConflict, err.to_string())
                    .exit()
            }
            config_gen::generate_config(args)
        }
        Commands::GenerateHostMonitoring(args) => {
            host_monitoring_gen::generate_host_monitoring_config(args)
        }
    };

    if let Err(err) = result {
        error!("Command failed: {err}");
        return err.into();
    }

    ExitCode::SUCCESS
}
