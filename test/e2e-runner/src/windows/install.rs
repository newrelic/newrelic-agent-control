use crate::tools::test::TestResult;

use super::powershell::exec_powershell_command;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use tracing::info;

const INSTALL_SCRIPT_NAME: &str = "install.ps1";

/// Extracts a zip file to a temporary directory using PowerShell's Expand-Archive.
fn unzip_to_temp(zip_path: &str) -> TestResult<TempDir> {
    // Create a temporary directory
    let temp_dir = TempDir::with_prefix("agent-control-install-")?;

    // Use PowerShell's Expand-Archive to extract the zip
    let cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        zip_path,
        temp_dir.path().display()
    );

    exec_powershell_command(&cmd, "failed to extract zip file")?;

    Ok(temp_dir)
}

/// Installs agent-control using the install.ps1 PowerShell script.
pub fn install_agent_control(package_path: &str, service_overwrite: bool) -> TestResult<()> {
    info!("Installing Agent Control");
    // Check if the package file exists
    if !Path::new(package_path).exists() {
        return Err(format!("package file not found at {:?}", package_path).into());
    }

    // Extract the zip file to a temporary directory
    let temp_dir = unzip_to_temp(package_path)?;

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
    let output = Command::new("powershell.exe")
        .args(&args)
        .current_dir(temp_dir.path())
        .output()?;

    if !output.status.success() {
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{:?} execution failed with {}: {}\n{}",
            INSTALL_SCRIPT_NAME, exit_code, stdout, stderr
        )
        .into());
    }

    info!("Installation completed successfully. Showing installation output");
    println!("---\n{}\n---", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
