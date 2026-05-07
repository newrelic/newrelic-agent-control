use tracing::info;

use crate::linux::bash::exec_bash_command;

pub const STATUS_RUNNING: &str = "active";

/// Restarts a service using systemctl
pub fn restart_service(service_name: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("systemctl restart {service_name}");
    let _ = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not restart the '{service_name}' service: {err}"));
}

/// Restarts a service using systemctl and waits for it to reach the expected status
pub fn restart_service_and_wait(service_name: &str, expected_status: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("systemctl restart {service_name}");
    let _ = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not restart the '{service_name}' service: {err}"));

    // Wait for service to reach expected status
    use std::thread::sleep;
    use std::time::Duration;

    for i in 0..30 {
        let current_status = get_service_status(service_name);
        if current_status == expected_status {
            info!(
                service = service_name,
                status = expected_status,
                "Service reached expected status"
            );
            return;
        }
        if i % 5 == 0 {
            info!(
                service = service_name,
                current_status = current_status,
                expected_status = expected_status,
                "Waiting for service to reach expected status"
            );
        }
        sleep(Duration::from_secs(1));
    }
    let final_status = get_service_status(service_name);

    // Show service logs when it fails
    info!("Service failed to reach expected status, showing logs");
    let log_cmd = format!("journalctl -u {service_name} -n 50 --no-pager");
    if let Ok(logs) = exec_bash_command(&log_cmd) {
        info!("Service logs:\n{}", logs);
    }

    panic!(
        "Service {service_name} did not reach status {expected_status} within 30 seconds. Final status: {final_status}"
    );
}

/// Gets the current status of a service using systemctl
pub fn get_service_status(service_name: &str) -> String {
    let cmd = format!("systemctl show --property=ActiveState {service_name} | cut -d= -f2");
    let output = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not get status of '{service_name}' service: {err}"));

    // Extract stdout from the formatted output
    output
        .lines()
        .find(|line| line.starts_with("Stdout: "))
        .and_then(|line| line.strip_prefix("Stdout: "))
        .unwrap_or("")
        .trim()
        .to_string()
}
