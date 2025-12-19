use crate::tools::test::TestResult;

use super::service;
use std::fs;
use tracing::warn;

const AGENT_CONTROL_DIRS: &[&str] = &[
    r"C:\Program Files\New Relic\newrelic-agent-control\",
    r"C:\ProgramData\New Relic\newrelic-agent-control\",
];

/// Cleans up the agent-control installation by stopping the service and removing directories.
pub fn cleanup(service_name: &str) -> TestResult<()> {
    if let Err(e) = service::stop_service(service_name) {
        return Err(format!("failed to stop service: {}", e).into());
    }

    for dir in AGENT_CONTROL_DIRS {
        match fs::remove_dir_all(dir) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!(directory = dir, "Directory not found");
            }
            Err(e) => {
                return Err(format!("could not remove {:?}: {}", dir, e).into());
            }
        }
    }

    Ok(())
}
