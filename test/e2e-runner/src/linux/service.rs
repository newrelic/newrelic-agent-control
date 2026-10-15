use std::thread::sleep;
use std::time::Duration;
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

/// Stops a service using systemctl, which blocks until the unit has stopped.
pub fn stop_service(service_name: &str) {
    info!(service = service_name, "Stopping service");
    let cmd = format!("systemctl stop {service_name}");
    let _ = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not stop the '{service_name}' service: {err}"));
}

/// Restarts a service using systemctl and waits for it to reach the expected status
pub fn restart_service_and_wait(service_name: &str, expected_status: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("systemctl restart {service_name}");
    let _ = exec_bash_command(&cmd)
        .unwrap_or_else(|err| panic!("could not restart the '{service_name}' service: {err}"));

    // Wait for service to reach expected status
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

    // Final check in case the service reached expected status right after the loop
    let final_status = get_service_status(service_name);
    if final_status == expected_status {
        info!(
            service = service_name,
            status = expected_status,
            "Service reached expected status on final check"
        );
        return;
    }

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

/// Reads a single systemd unit property via `systemctl show --property=<prop>`.
/// Returns `None` if the command fails or the output is malformed.
fn get_systemctl_property(service_name: &str, property: &str) -> Option<String> {
    let cmd = format!("systemctl show --property={property} {service_name} | cut -d= -f2");
    exec_bash_command(&cmd)
        .ok()
        .and_then(|out| {
            out.lines()
                .find(|l| l.starts_with("Stdout: "))
                .map(|l| l.to_owned())
        })
        .and_then(|l| l.strip_prefix("Stdout: ").map(|s| s.trim().to_owned()))
}

/// Gets the current status of a service using systemctl
pub fn get_service_status(service_name: &str) -> String {
    get_systemctl_property(service_name, "ActiveState").unwrap_or_default()
}

pub const ENABLED: &str = "enabled";

/// Returns the number of times systemd has automatically restarted the service since it was
/// last manually started. Resets to 0 on `systemctl start` / `systemctl restart`.
pub fn get_auto_restart_count(service_name: &str) -> u32 {
    get_systemctl_property(service_name, "NRestarts")
        .and_then(|s| s.parse().ok())
        .expect("could not read NRestarts from systemctl output")
}

/// Returns `true` if systemd stopped restarting the service because the burst rate limit
/// was reached (`StartLimitHit=yes`). Always `false` when `StartLimitIntervalSec=0`.
pub fn is_start_limit_hit(service_name: &str) -> bool {
    get_systemctl_property(service_name, "StartLimitHit")
        .map(|s| s == "yes")
        .expect("could not read StartLimitHit from systemctl output")
}

/// Gets whether a service is enabled to start on boot (`systemctl show --property=UnitFileState`).
pub fn get_unit_file_state(service_name: &str) -> String {
    get_systemctl_property(service_name, "UnitFileState").unwrap_or_default()
}

/// Returns the systemd-reported result of the unit's last run (`systemctl show --property=Result`):
/// `"success"` for a clean exit, or `"exit-code"`/`"signal"`/`"timeout"`/`"watchdog"`/`"core-dump"`
/// when the unit was killed or failed to stop within `TimeoutStopSec`.
pub fn get_service_result(service_name: &str) -> String {
    get_systemctl_property(service_name, "Result").unwrap_or_default()
}

/// Sends SIGKILL to the unit's main process
pub fn kill_main_process(service_name: &str) {
    let pid: u32 = get_systemctl_property(service_name, "MainPID")
        .and_then(|s| s.parse().ok())
        .expect("could not read MainPID from systemctl output");
    // systemd reports 0 when the unit has no active process (stopped, failed or between automatic
    // restarts), and `kill -9 0` would signal the whole process group, including the test runner.
    assert_ne!(pid, 0, "service {service_name} has no main process to kill");
    exec_bash_command(&format!("kill -9 {pid}"))
        .unwrap_or_else(|err| panic!("could not kill process {pid}: {err}"));
}
