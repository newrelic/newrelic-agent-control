use crate::{WindowsCli, WindowsScenarios, init_logging};
use clap::Parser;

pub mod install;
pub mod scenarios;

mod cleanup;
mod health;
mod powershell;
mod service;
mod utils;

const DEFAULT_CONFIG_PATH: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\local-data\agent-control\local_config.yaml";

const DEFAULT_NR_INFRA_PATH: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\local-data\nr-infra\local_config.yaml";

const DEFAULT_LOG_PATH: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\logs\newrelic-agent-control.log";

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
    }
}
