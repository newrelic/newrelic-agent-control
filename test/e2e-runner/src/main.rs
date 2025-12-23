mod linux;
mod tools;
mod windows;

use clap::Parser;
use std::process;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::tools::test::TestResult;

fn main() {
    // Using `cfg!` instead of `#[cfg]` to make development easier
    if cfg!(target_os = "windows") {
        run_windows_e2e();
    }
    if cfg!(target_os = "linux") {
        run_linux_e2e();
    }
    error!("Unsupported OS -- only Linux and Windows are supported");
    process::exit(1);
}

#[derive(Debug, clap::Subcommand)]
enum LinuxScenarios {
    InfraAgent,
}

#[derive(Debug, clap::Subcommand)]
enum WindowsScenarios {
    Install(windows::scenarios::installation::Args),
}

#[derive(Parser)]
#[command(
    name = "e2e-runner",
    about = "E2E Test Runner for newrelic-agent-control on Linux",
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on Linux.\n\n\
                  PREREQUISITES:\n\
                  - Debian package manager
                  - Systemctl
                  - Run as root"
)]
struct LinuxCli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    scenario: LinuxScenarios,
}

#[derive(Parser)]
#[command(
    name = "e2e-runner",
    about = "E2E Test Runner for newrelic-agent-control on Windows",
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on Windows.\n\n\
                  PREREQUISITES:\n\
                  - Run as Administrator"
)]
struct WindowsCli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    scenario: WindowsScenarios,
}

fn run_linux_e2e() {
    let cli = LinuxCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    let result = match cli.scenario {
        LinuxScenarios::InfraAgent => {
            // TODO args
            linux::scenarios::infra_agent::test_installation_with_infra_agent()
        }
    };

    handle_result(result);
}

fn run_windows_e2e() {
    let cli = WindowsCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    let result = match cli.scenario {
        WindowsScenarios::Install(args) => {
            windows::scenarios::installation::test_installation(args)
        }
    };
    handle_result(result);
}

fn init_logging(level: &str) {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::new(level))
        .init();
}

fn handle_result(result: TestResult<()>) {
    match result {
        Ok(_) => {
            info!("Test completed successfully");
        }
        Err(err) => {
            error!("Test failed: {err}");
            process::exit(1);
        }
    }
}
