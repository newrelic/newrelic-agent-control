use crate::tools::test::TestResult;

use super::powershell::exec_powershell_command;
use std::thread;
use std::time::Duration;
use tracing::info;

/// Checks if a Windows service is running using PowerShell.
pub fn check_service_running(service_name: &str) -> TestResult<()> {
    match get_service_status(service_name) {
        Ok(status) if status == "Running" => {
            info!(service = service_name, "Windows service is running");
            Ok(())
        }
        Ok(status) => Err(format!(
            "service {:?} is not running. Status: {}",
            service_name, status
        )
        .into()),
        Err(e) => Err(format!("failed to get status of service {:?}: {}", service_name, e).into()),
    }
}

/// Gets the current status of a Windows service as a string using PowerShell.
pub fn get_service_status(service_name: &str) -> TestResult<String> {
    let cmd = format!("(Get-Service -Name '{}').Status", service_name);
    exec_powershell_command(&cmd, "failed to execute PowerShell script")
}

/// Restarts a Windows service using PowerShell.
pub fn restart_service(service_name: &str) -> TestResult<()> {
    info!(service = service_name, "Restarting service");
    let cmd = format!("Restart-Service -Name '{}' -Force", service_name);
    exec_powershell_command(&cmd, "failed to restart service")?;

    // Wait a moment for the service to fully restart
    info!("Waiting for service to restart...");
    thread::sleep(Duration::from_secs(5));

    check_service_running(service_name)?;

    info!(service = service_name, "Service restarted successfully");
    Ok(())
}

/// Stops a Windows service using PowerShell.
pub fn stop_service(service_name: &str) -> TestResult<()> {
    let cmd = format!("Stop-Service -Name '{}' -Force", service_name);
    exec_powershell_command(&cmd, "failed to stop service")?;
    Ok(())
}
