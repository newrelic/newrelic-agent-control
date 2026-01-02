use std::process::Command;

use crate::tools::test::TestResult;

/// Executes a PowerShell command and returns its output or an error.
pub fn exec_powershell_command(command: &str) -> TestResult<String> {
    let mut cmd = Command::new("powershell.exe");
    let cmd = cmd.arg("-Command").arg(command);
    exec_powershell_cmd(cmd)
        .map_err(|err| format!("failed to execute command '{command}': {err}").into())
}

/// Executes the provided [Command] and resturs its output or an error.
pub fn exec_powershell_cmd(cmd: &mut Command) -> TestResult<String> {
    let output = cmd.output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(format!("command failed\nStdout: {stdout}\nStderr: {stderr}").into());
    }

    Ok(stdout)
}
