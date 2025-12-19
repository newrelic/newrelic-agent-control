use std::process::Command;

use crate::tools::test::TestResult;

/// Executes a PowerShell command and returns its output or an error.
pub fn exec_powershell_command(command: &str, err_context: &str) -> TestResult<String> {
    let output = Command::new("powershell.exe")
        .arg("-Command")
        .arg(command)
        .output()
        .map_err(|e| format!("{}: failed to execute command: {}", err_context, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(
            format!("{err_context}: command failed\nStdout: {stdout}\nStderr: {stderr}",).into(),
        );
    }

    Ok(stdout)
}
