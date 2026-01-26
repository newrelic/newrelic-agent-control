use crate::common::test::{TestResult, retry};

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
pub fn restart_service(service_name: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("Restart-Service -Name '{}' -Force", service_name);
    exec_ps(&cmd).unwrap_or_else(|err| panic!("could not restart '{service_name}' service: {err}"));

    // Wait a moment for the service to fully restart
    info!("Waiting for service to restart...");
    thread::sleep(Duration::from_secs(5));

    check_service_status(service_name, STATUS_RUNNING).expect("service must be running");

    info!(service = service_name, "Service restarted successfully");
}

/// Stops a Windows service using PowerShell.
pub fn stop_service(service_name: &str) {
    let cmd = format!("Stop-Service -Name '{}' -Force", service_name);
    exec_ps(&cmd).unwrap_or_else(|err| panic!("could not stop '{service_name} service: {err}'"));

    retry(30, Duration::from_secs(5), "check service stopped", || {
        check_service_status(service_name, STATUS_STOPPED)
    })
    .expect("check service stop failed");
}
