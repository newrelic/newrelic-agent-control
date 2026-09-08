use crate::common::test::{TestResult, retry_panic};

use super::powershell::exec_ps;
use std::thread;
use std::time::Duration;
use tracing::info;

pub const STATUS_STOPPED: &str = "Stopped";
pub const STATUS_RUNNING: &str = "Running";

/// Checks if a Windows service is running using PowerShell.
pub fn check_service_status(service_name: &str, service_status: &str) -> TestResult<()> {
    info!("Checking Windows service status");
    match get_service_status(service_name) {
        status if status == service_status => Ok(()),
        status => {
            Err(format!("service {service_name} is not {service_status}. Status: {status}").into())
        }
    }
}

/// Gets the current status of a Windows service as a string using PowerShell.
fn get_service_status(service_name: &str) -> String {
    let cmd = format!("(Get-Service -Name '{}').Status", service_name);
    let result = exec_ps(&cmd).unwrap_or_else(|err| panic!("could not get service status: {err}"));
    let stdout_line = result
        .lines()
        .find(|line| line.starts_with("Stdout"))
        .expect("Result from powershell command should have an \"Stdout\" line");
    let stdout = stdout_line
        .split(":")
        .last()
        .expect("Stdout line should contain a colon");

    stdout.trim().to_string()
}

/// Restarts a Windows service using PowerShell.
pub fn restart_service(service_name: &str, expected_service_status: &str) {
    info!(service = service_name, "Restarting service");

    let cmd = format!("Restart-Service -Name '{service_name}' -Force");
    let result = exec_ps(&cmd);
    // When AC is expected to fail, Restart-Service may exit with a non-zero code because the
    // service won't stay running. That's fine because the real assertion is the status check below.
    if expected_service_status == STATUS_RUNNING {
        result.unwrap_or_else(|err| panic!("could not restart '{service_name}' service: {err}"));
    }

    // Wait a moment for the service to fully restart
    info!("Waiting for service to restart...");
    thread::sleep(Duration::from_secs(5));

    check_service_status(service_name, expected_service_status)
        .expect("service must be on the status provided");

    info!(
        service = service_name,
        status = expected_service_status,
        "Service status matches the expectation"
    );
}

/// Returns the total count of Windows Event Log entries recording that the SCM took a
/// restart recovery action for the service (Event ID 7031: "service terminated unexpectedly").
/// Call this before and after inducing failures; the delta is the number of restarts.
pub fn get_scm_restart_count(service_name: &str) -> u32 {
    let cmd = format!(
        r#"(Get-WinEvent -FilterHashtable @{{LogName='System'; Id=7031}} -ErrorAction SilentlyContinue | Where-Object {{$_.Properties.Value -like '*{service_name}*'}} | Measure-Object).Count"#
    );
    exec_ps(&cmd)
        .ok()
        .and_then(|out| {
            out.lines()
                .find(|l| l.starts_with("Stdout"))
                .map(|l| l.to_owned())
        })
        .and_then(|l| l.split(':').next_back().map(|s| s.trim().to_owned()))
        .and_then(|s| s.parse().ok())
        .expect("could not read Event 7031 count from PowerShell output")
}

/// Stops a Windows service using PowerShell.
pub fn stop_service(service_name: &str) {
    let cmd = format!("Stop-Service -Name '{}' -Force", service_name);
    exec_ps(&cmd).unwrap_or_else(|err| panic!("could not stop '{service_name} service: {err}'"));

    retry_panic(30, Duration::from_secs(5), "check service stopped", || {
        check_service_status(service_name, STATUS_STOPPED)
    });
}
