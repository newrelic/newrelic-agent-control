use crate::{LinuxCli, LinuxScenarios, init_logging};
use clap::Parser;

pub mod install;
pub mod scenarios;

mod bash;
mod service;

const DEFAULT_AC_CONFIG_PATH: &str =
    "/etc/newrelic-agent-control/local-data/agent-control/local_config.yaml";

pub fn local_config_path(agent_id: &str) -> String {
    format!(r"/etc/newrelic-agent-control/local-data/{agent_id}/local_config.yaml")
}

const DEFAULT_LOG_PATH: &str = "/var/log/newrelic-agent-control/newrelic-agent-control.log";

const SERVICE_NAME: &str = "newrelic-agent-control";

/// Run Linux e2e corresponding scenario which will panic on failure
pub fn run_linux_e2e() {
    let cli = LinuxCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    match cli.scenario {
        LinuxScenarios::InfraAgent(args) => {
            scenarios::infra_agent::test_installation_with_infra_agent(args);
        }
        LinuxScenarios::EBPFAgent(args) => {
            scenarios::ebpf_agent::test_ebpf_agent(args);
        }
        LinuxScenarios::NrdotAgent(args) => {
            scenarios::nrdot_agent::test_nrdot_agent(args);
        }
        LinuxScenarios::RemoteConfig(args) => {
            scenarios::remote_config::test_remote_config_is_applied(args);
        }
        LinuxScenarios::Proxy(args) => {
            scenarios::proxy::test_agent_control_proxy(args);
        }
    };
}
