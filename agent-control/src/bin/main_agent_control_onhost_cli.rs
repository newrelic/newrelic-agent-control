use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use newrelic_agent_control::cli::on_host::migrate_folders;
use newrelic_agent_control::cli::on_host::uninstall::{self, UninstallArgs};
use newrelic_agent_control::cli::on_host::update::{self, UpdateArgs};
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
    /// Uninstall Agent Control and all managed agents from this host.
    ///
    /// Stops the service, removes all OCI-installed managed agent packages, removes
    /// binaries, and (unless --keep-config) removes configuration and state directories.
    Uninstall(UninstallArgs),
    /// Update Agent Control to the specified version using the OCI registry.
    ///
    /// This bypasses Fleet Control release channels and is intended only as a break-glass
    /// operation for installations not managed by Fleet Control.
    Update(UpdateArgs),
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
            Ok(params) => config_gen::generate(params),
            Err(err) => {
                let mut cmd = Cli::command();
                cmd.error(ErrorKind::ArgumentConflict, err.to_string())
                    .exit()
            }
        },
        Commands::FilesBackwardsCompatibilityMigrationFromV120 => migrate_folders::migrate(),
        Commands::Uninstall(args) => uninstall::run(args),
        Commands::Update(args) => update::run(args),
    };

    if let Err(err) = result {
        error!("Command failed: {err}");
        return err.into();
    }

    ExitCode::SUCCESS
}
