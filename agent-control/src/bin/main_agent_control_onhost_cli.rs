use std::process::ExitCode;

use clap::Parser;
use newrelic_agent_control::cli::{logs, on_host::config_gen};
use tracing::Level;

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
#[derive(Debug, clap::Subcommand)]
enum Commands {
    // Generate Agent Control configuration according to the provided configuration data.
    GenerateConfig(config_gen::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let tracer = logs::init(cli.cli_log_level);
    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {err}");
        return err.into();
    }

    let result = match cli.command {
        Commands::GenerateConfig(inputs) => config_gen::generate_config(inputs),
    };

    if let Err(err) = result {
        return err.into();
    }

    ExitCode::SUCCESS
}
