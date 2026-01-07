pub mod scenarios;

pub mod bash;
pub mod install;
pub mod service;

pub const DEFAULT_CONFIG_PATH: &str =
    "/etc/newrelic-agent-control/local-data/agent-control/local_config.yaml";

pub const DEFAULT_LOG_PATH: &str = "/var/log/newrelic-agent-control/newrelic-agent-control.log";

pub const SERVICE_NAME: &str = "newrelic-agent-control";
