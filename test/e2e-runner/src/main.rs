mod tools;
mod windows;

use clap::Parser;
use std::process;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

// TODO: adapt to handle windows and linux scenarios

#[derive(Debug, clap::Subcommand)]
enum Scenario {
    Install(windows::scenarios::installation::Args),
}

#[derive(Parser)]
#[command(
    name = "e2e-runner",
    about = "E2E Test Runner for newrelic-agent-control",
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on Windows.\n\n\
                  PREREQUISITES:\n\
                  - Must be run as Administrator on Windows"
)]
struct Cli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    scenario: Scenario,
}

fn main() {
    let cli = Cli::parse();

    // Initialize tracing subscriber with CLI log level
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::new(&cli.log_level))
        .init();

    // Check that we are running on Windows
    if cfg!(not(target_os = "windows")) {
        error!("Windows e2e tests are only supported on Windows");
        process::exit(1);
    }

    // Run the requested test
    let result = match cli.scenario {
        Scenario::Install(args) => windows::scenarios::installation::test_installation(args),
    };

    // Handle the result
    match result {
        Ok(_) => {
            info!("Test completed successfully");
        }
        Err(e) => {
            error!("Test failed: {}", e);
            process::exit(1);
        }
    }
}
