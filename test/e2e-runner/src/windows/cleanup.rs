use crate::tools::remove_dirs;

use super::service;

const AGENT_CONTROL_DIRS: &[&str] = &[
    r"C:\Program Files\New Relic\newrelic-agent-control\",
    r"C:\ProgramData\New Relic\newrelic-agent-control\",
];

/// Cleans up the agent-control installation by stopping the service and removing directories.
pub fn cleanup(service_name: &str) {
    service::stop_service(service_name);
    remove_dirs(AGENT_CONTROL_DIRS).expect("expected directories to be removed");
}
