use tracing::info;

use crate::linux::bash::exec_bash_command;

/// Restarts a service using systemctl
pub fn restart_service(service_name: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("systemctl restart {service_name}");
    let _ = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not restart the '{service_name}' service: {err}"));
}
