mod common;
mod linux;
mod macos;
mod windows;

use crate::common::{FleetControlApiArgs, FleetControlInstallationArgs, InstallationArgs};
use clap::Parser;
use std::process;
use tracing_subscriber::EnvFilter;

fn main() {
    if cfg!(target_os = "windows") {
        windows::run_windows_e2e()
    } else if cfg!(target_os = "linux") {
        linux::run_linux_e2e()
    } else if cfg!(target_os = "macos") {
        macos::run_macos_e2e()
    } else {
        panic!("Unsupported OS -- only Linux, Windows, and macOS are supported")
    }
    process::exit(0);
}

#[derive(Debug, clap::Subcommand)]
enum LinuxScenarios {
    /// Local installation of Agent Control with Infrastructure Agent. It checks that the infra-agent eventually reports data.
    InfraAgent(InstallationArgs),
    /// Local installation of Agent Control with eBPF agent. Uses the infra agent to generate traffic and checks that
    /// the eBPF agent reports data.
    EBPFAgent(InstallationArgs),
    /// Local installation of Agent Control with NRDot. It checks that nr-dot eventually reports data.
    NrdotAgent(InstallationArgs),
    /// Checks that remote configuration for a sub-agent has been applied.
    RemoteConfig(InstallationArgs),
    /// Checks that the Agent Control proxy support works as expected. It uses mitproxy as a docker service.
    Proxy(InstallationArgs),
    /// Tests Fleet Control integration by installing Agent Control with fleet configuration and triggering Fleet Control tests.
    ///
    /// This relies on polling certain fixed Fleet Control endpoints, failing if the response is not expected or a timeout is reached.
    FleetControl(FleetControlInstallationArgs),
    /// Triggers Fleet Control tests via API and polls for completion (without installing Agent Control).
    ///
    /// This is useful when Agent Control is already deployed and you only need to trigger and monitor Fleet Control tests.
    /// Requires --fleet-id and --fleet-control-token arguments.
    FleetControlApi(FleetControlApiArgs),
    /// Tests self-update functionality by installing latest released Agent Control, and verifying that AC updates itself,
    /// when instructed via OpAMP, to the current compiled version (pushed to local registry).
    SelfUpdateLatestToCurrent(InstallationArgs),
    /// Tests self-update functionality by installing Agent Control from current branch and verifying that AC updates itself,
    /// when instructed via OpAMP, to the latest published tag.
    SelfUpdateCurrentToLatest(InstallationArgs),
}

#[derive(Debug, clap::Subcommand)]
enum WindowsScenarios {
    /// Simple installation of Agent Control on Windows with an Infrastructure Agent.
    InfraAgent(InstallationArgs),
    /// Simple installation of Agent Control on Windows with NRDOT Agent.
    Nrdot(InstallationArgs),
    Proxy(InstallationArgs),
    /// Checks that remote configuration for a sub-agent has been applied on Windows.
    RemoteConfig(InstallationArgs),
    /// Tests that remote configuration for infra-agent has been applied via fleet management. Includes new version download.
    /// Starts with a local-only installation of AC, then updates the installation to include fleet configuration that should
    /// trigger a sub-agent update.
    SwitchInfraAgentVersion(InstallationArgs),
    /// Tests self-update functionality by installing latest released Agent Control, and verifying that AC updates itself,
    /// when instructed via OpAMP, to the current compiled version (pushed to local registry).
    SelfUpdateLatestToCurrent(InstallationArgs),
    /// Tests self-update functionality by installing Agent Control from current branch and verifying that AC updates itself,
    /// when instructed via OpAMP, to the latest published tag.
    SelfUpdateCurrentToLatest(InstallationArgs),
    /// Simple installation of Agent Control on Windows with update to wrong and correct config
    /// to test service stop and start.
    WrongConfig(InstallationArgs),
    /// Tests Fleet Control integration by installing Agent Control with fleet configuration and triggering Fleet Control tests.
    ///
    /// This relies on polling certain fixed Fleet Control endpoints, failing if the response is not expected or a timeout is reached.
    FleetControl(FleetControlInstallationArgs),
    /// Triggers Fleet Control tests via API and polls for completion (without installing Agent Control).
    ///
    /// This is useful when Agent Control is already deployed and you only need to trigger and monitor Fleet Control tests.
    /// Requires --fleet-id and --fleet-control-token arguments.
    FleetControlApi(FleetControlApiArgs),
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

#[derive(Debug, clap::Subcommand)]
enum MacOSScenarios {
    /// Triggers Fleet Control tests via API and polls for completion (without installing Agent Control).
    ///
    /// This is useful when Agent Control is already deployed (e.g., in a minikube cluster) and you only need to trigger and monitor Fleet Control tests from macOS.
    /// Requires --fleet-id and --fleet-control-token arguments.
    FleetControlApi(FleetControlApiArgs),
}

#[derive(Parser)]
#[command(
    name = "e2e-runner",
    about = "E2E Test Runner for newrelic-agent-control on macOS",
    long_about = "This tool runs end-to-end tests for newrelic-agent-control on macOS."
)]
struct MacOSCli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    scenario: MacOSScenarios,
}

fn init_logging(level: &str) {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::new(level))
        .init();
}
