use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use newrelic_agent_control::cli::on_host::migrate_folders;
use newrelic_agent_control::cli::{common::logs, on_host::config_gen};
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
    /// Generate Agent Control configuration according to the provided configuration data.
    /// It generates the AC config, and it creates the system identity
    GenerateConfig(config_gen::Args),
    /// Migrates legacy on-host directories (>v1.4.0) to the new layout. Intended to be run by post-installation package scripts only.
    FilesBackwardsCompatibilityMigrationFromV120,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let tracer = logs::init(cli.cli_log_level);
    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {err}");
        return err.into();
    }

    let result = match cli.command {
        Commands::GenerateConfig(args) => match args.validate() {
            Ok(inputs) => config_gen::generate(inputs),
            Err(err) => {
                let mut cmd = Cli::command();
                cmd.error(ErrorKind::ArgumentConflict, err.to_string())
                    .exit()
            }
        },
        Commands::FilesBackwardsCompatibilityMigrationFromV120 => migrate_folders::migrate(),
    };

    if let Err(err) = result {
        error!("Command failed: {err}");
        return err.into();
    }

    ExitCode::SUCCESS
}
