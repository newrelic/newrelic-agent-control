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
    /// Local installation of Agent Control with Infrastructure Agent. It checks that the infra-agent eventually reports data.
    InfraAgent(linux::install::Args),
    /// Local installation of Agent Control with eBPF agent. Uses the infra agent to generate traffic and checks that
    /// the eBPF agent reports data.
    EBPFAgent(linux::install::Args),
    /// Migration of an Infrastructure Agent installation. It spawns a mysql docker service, reports mysql metrics
    /// through the infra-agent and checks that metrics keep reporting after migration.
    Migration(linux::install::Args),
    /// Local installation of Agent Control with NRDot. It checks that nr-dot eventually reports data.
    NrdotAgent(linux::install::Args),
    /// Checks that remote configuration for a sub-agent has been applied.
    RemoteConfig(linux::install::Args),
    /// Checks that the Agent Control proxy support works as expected. It uses mitproxy as a docker service.
    Proxy(linux::install::Args),
}

#[derive(Debug, clap::Subcommand)]
enum WindowsScenarios {
    /// Simple installation of Agent Control on Windows
    Install(windows::install::Args),
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
        LinuxScenarios::InfraAgent(args) => {
            linux::scenarios::infra_agent::test_installation_with_infra_agent(args);
        }
        LinuxScenarios::EBPFAgent(args) => {
            linux::scenarios::ebpf_agent::test_ebpf_agent(args);
        }
        LinuxScenarios::Migration(args) => {
            linux::scenarios::migration::test_migration(args);
        }
        LinuxScenarios::NrdotAgent(args) => {
            linux::scenarios::nrdot_agent::test_nrdot_agent(args);
        }
        LinuxScenarios::RemoteConfig(args) => {
            linux::scenarios::remote_config::test_remote_config_is_applied(args);
        }
        LinuxScenarios::Proxy(args) => {
            linux::scenarios::proxy::test_agent_control_proxy(args);
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
