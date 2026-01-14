pub mod install;
pub mod scenarios;

mod cleanup;
mod health;
mod powershell;
mod service;

const DEFAULT_CONFIG_PATH: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\local-data\agent-control\local_config.yaml";

const DEFAULT_LOG_PATH: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\logs\newrelic-agent-control.log";
