mod linux;
mod tools;
mod windows;

use clap::Parser;
use std::process;
use tracing_subscriber::EnvFilter;

fn main() {
    // Using `cfg!` instead of `#[cfg]` to make development easier
    if cfg!(target_os = "windows") {
        run_windows_e2e()
    } else if cfg!(target_os = "linux") {
        run_linux_e2e()
    } else {
        panic!("Unsupported OS -- only Linux and Windows are supported");
    }
    process::exit(0);
}

#[derive(Debug, clap::Subcommand)]
enum LinuxScenarios {
    /// Local installation of Agent Control with Infrastructure. It checks that the infra-agent eventually reports data.
    InfraAgent(linux::install::Args),
}

#[derive(Debug, clap::Subcommand)]
enum WindowsScenarios {
    /// Simple installation of Agent Control on Windows
    Install(windows::scenarios::installation::Args),
}

#[derive(Parser)]
#[command(
    name = "e2e-runner",
    about = "E2E Test Runner for newrelic-agent-control on Linux",
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on Linux.\n
PREREQUISITES:
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
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on Windows.\n
PREREQUISITES:
- Run as Administrator"
)]
struct WindowsCli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    scenario: WindowsScenarios,
}

/// Run Linux e2e corresponding scenario which will panic on failure
fn run_linux_e2e() {
    let cli = LinuxCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    match cli.scenario {
        LinuxScenarios::InfraAgent(recipe_data) => {
            linux::scenarios::infra_agent::test_installation_with_infra_agent(recipe_data)
        }
    };
}

/// Run Windows e2e corresponding scenario which will panic on failure
fn run_windows_e2e() {
    let cli = WindowsCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    match cli.scenario {
        WindowsScenarios::Install(args) => {
            windows::scenarios::installation::test_installation(args);
        }
    }
}

fn init_logging(level: &str) {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::new(level))
        .init();
}
