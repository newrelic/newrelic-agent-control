mod common;
mod linux;
mod windows;

use crate::common::Args;
use clap::Parser;
use std::process;
use tracing_subscriber::EnvFilter;

fn main() {
    // Using `cfg!` instead of `#[cfg]` to make development easier
    if cfg!(target_os = "windows") {
        windows::run_windows_e2e()
    } else if cfg!(target_os = "linux") {
        linux::run_linux_e2e()
    } else {
        panic!("Unsupported OS -- only Linux and Windows are supported");
    }
    process::exit(0);
}

#[derive(Debug, clap::Subcommand)]
enum LinuxScenarios {
    /// Local installation of Agent Control with Infrastructure Agent. It checks that the infra-agent eventually reports data.
    InfraAgent(Args),
    /// Local installation of Agent Control with eBPF agent. Uses the infra agent to generate traffic and checks that
    /// the eBPF agent reports data.
    EBPFAgent(Args),
    /// Migration of an Infrastructure Agent installation. It spawns a mysql docker service, reports mysql metrics
    /// through the infra-agent and checks that metrics keep reporting after migration.
    Migration(Args),
    /// Local installation of Agent Control with NRDot. It checks that nr-dot eventually reports data.
    NrdotAgent(Args),
    /// Checks that remote configuration for a sub-agent has been applied.
    RemoteConfig(Args),
    /// Checks that the Agent Control proxy support works as expected. It uses mitproxy as a docker service.
    Proxy(Args),
}

#[derive(Debug, clap::Subcommand)]
enum WindowsScenarios {
    /// Simple installation of Agent Control on Windows with an Infrastructure Agent
    InfraAgent(Args),
    Proxy(Args),
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

fn init_logging(level: &str) {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::new(level))
        .init();
}
