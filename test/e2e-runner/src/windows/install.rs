use crate::windows::powershell::exec_powershell_cmd;

use super::powershell::exec_powershell_command;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use tracing::info;

const INSTALL_SCRIPT_NAME: &str = "install.ps1";

/// Extracts a zip file to a temporary directory using PowerShell's Expand-Archive, panics on failure.
fn unzip_to_temp(zip_path: &str) -> TempDir {
    // Create a temporary directory
    let temp_dir =
        TempDir::with_prefix("agent-control-install-").expect("could not create temp dir");

    // Use PowerShell's Expand-Archive to extract the zip
    let cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        zip_path,
        temp_dir.path().display()
    );

    exec_powershell_command(&cmd).unwrap_or_else(|err| panic!("could not unzip to temp: {err}"));

    temp_dir
}

/// Installs agent-control using the install.ps1 PowerShell script, panics on failure
pub fn install_agent_control(package_path: &str, service_overwrite: bool) {
    info!("Installing Agent Control");
    // Check if the package file exists
    if !Path::new(package_path).exists() {
        panic!("package file not found at {:?}", package_path);
    }

    // Extract the zip file to a temporary directory
    let temp_dir = unzip_to_temp(package_path);

    let install_script = temp_dir.path().join(INSTALL_SCRIPT_NAME);

    // Build PowerShell command arguments
    let mut args = vec![
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        install_script.to_string_lossy().to_string(),
    ];
    if service_overwrite {
        args.push("-ServiceOverwrite".to_string());
    }

    // Execute PowerShell command
    let mut cmd = Command::new("powershell.exe");
    let cmd = cmd.args(&args).current_dir(temp_dir.path());

    let output = exec_powershell_cmd(cmd).unwrap_or_else(|err| {
        panic!("Failure executing ps1 installation script: {err}");
    });

    info!("Installation completed successfully");
    info!("---\n{output}\n---");
}
