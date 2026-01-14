pub mod install;
pub mod scenarios;

mod bash;
mod service;

const DEFAULT_CONFIG_PATH: &str =
    "/etc/newrelic-agent-control/local-data/agent-control/local_config.yaml";

const DEFAULT_LOG_PATH: &str = "/var/log/newrelic-agent-control/newrelic-agent-control.log";

const SERVICE_NAME: &str = "newrelic-agent-control";
