use crate::common::test::TestResult;
use crate::linux::bash::exec_bash_command;
use tracing::info;

/// Restarts a service using systemctl
pub fn restart_service(service_name: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("systemctl restart {service_name}");
    exec_bash_command(&cmd).unwrap_or_else(|err| {
        // Capture journalctl output to help diagnose why the service failed to start
        if let Ok(journal) =
            exec_bash_command(&format!("journalctl -xeu {service_name} --no-pager -n 50"))
        {
            info!("journalctl output:\n{journal}");
        }
        panic!("could not restart the '{service_name}' service: {err}")
    });
}

/// Checks if a service is active using systemctl
pub fn check_service_is_active(service_name: &str) -> TestResult<()> {
    let cmd = format!("systemctl is-active {service_name}");
    match exec_bash_command(&cmd) {
        Ok(output) => {
            let is_active = output
                .lines()
                .find(|line| line.starts_with("Stdout: "))
                .map(|line| line.trim_start_matches("Stdout: ").trim() == "active")
                .unwrap_or(false);
            if is_active {
                Ok(())
            } else {
                Err(format!("Service is not active: {}", output.trim()).into())
            }
        }
        Err(err) => Err(format!("Failed to check service status: {}", err).into()),
    }
}
