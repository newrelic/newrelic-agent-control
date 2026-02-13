use crate::{WindowsCli, WindowsScenarios, init_logging};
use clap::Parser;

pub mod install;
pub mod scenarios;

mod health;
mod powershell;
mod service;
mod utils;

const AGENT_CONTROL_DIRS: &[&str] = &[
    r"C:\Program Files\New Relic\newrelic-agent-control\",
    r"C:\ProgramData\New Relic\newrelic-agent-control\",
];

const DEFAULT_CONFIG_PATH: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\local-data\agent-control\local_config.yaml";

const DEFAULT_LOG_PATH: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\logs\newrelic-agent-control.log";

pub fn local_config_path(agent_id: &str) -> String {
    format!(
        r"C:\Program Files\New Relic\newrelic-agent-control\local-data\{agent_id}\local_config.yaml"
    )
}

/// Run Windows e2e corresponding scenario which will panic on failure
pub fn run_windows_e2e() {
    let cli = WindowsCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    match cli.scenario {
        WindowsScenarios::InfraAgent(args) => {
            scenarios::installation_infra_agent::test_infra_agent(args);
        }
        WindowsScenarios::Proxy(args) => {
            scenarios::proxy::test_proxy(args);
        }
        WindowsScenarios::Nrdot(args) => {
            scenarios::installation_nrdot::test_nrdot(args);
        }
        WindowsScenarios::WrongConfig(args) => {
            scenarios::service_wrong_config::test_service_restart_depending_on_config_correctness(
                args,
            );
        }
    }
}
