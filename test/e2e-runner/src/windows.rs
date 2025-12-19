pub mod scenarios;

pub mod cleanup;
pub mod health;
pub mod install;
pub mod powershell;
pub mod service;

pub const DEFAULT_CONFIG_PATH: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\local-data\agent-control\local_config.yaml";

pub const DEFAULT_LOG_PATH: &str =
    r"C:\ProgramData\New Relic\newrelic-agent-control\logs\newrelic-agent-control.log";
