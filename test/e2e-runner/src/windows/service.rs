use crate::tools::test::TestResult;

use super::powershell::exec_powershell_command;
use std::thread;
use std::time::Duration;
use tracing::info;

/// Checks if a Windows service is running using PowerShell.
pub fn check_service_running(service_name: &str) -> TestResult<()> {
    info!("Checking Windows service status");
    match get_service_status(service_name) {
        status if status == "Running" => {
            info!(service = service_name, "Windows service is running");
            Ok(())
        }
        status => Err(format!("service {service_name} is not running. Status: {status}").into()),
    }
}

/// Gets the current status of a Windows service as a string using PowerShell.
pub fn get_service_status(service_name: &str) -> String {
    let cmd = format!("(Get-Service -Name '{}').Status", service_name);
    exec_powershell_command(&cmd)
        .unwrap_or_else(|err| panic!("could not get service status: {err}"))
}

/// Restarts a Windows service using PowerShell.
pub fn restart_service(service_name: &str) {
    info!(service = service_name, "Restarting service");
    let cmd = format!("Restart-Service -Name '{}' -Force", service_name);
    exec_powershell_command(&cmd)
        .unwrap_or_else(|err| panic!("could not restart '{service_name}' service: {err}"));

    // Wait a moment for the service to fully restart
    info!("Waiting for service to restart...");
    thread::sleep(Duration::from_secs(5));

    check_service_running(service_name).expect("service must be running");

    info!(service = service_name, "Service restarted successfully");
}

/// Stops a Windows service using PowerShell.
pub fn stop_service(service_name: &str) {
    let cmd = format!("Stop-Service -Name '{}' -Force", service_name);
    exec_powershell_command(&cmd)
        .unwrap_or_else(|err| panic!("could not stop '{service_name} service: {err}'"));
}
