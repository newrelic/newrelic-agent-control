use std::process::Command;

use crate::tools::test::TestResult;

/// Executes a bash command and returns its output or an error.
pub fn exec_bash_command(command: &str) -> TestResult<String> {
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|err| format!("failed to execute command\n{command}\nerr: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(
            format!("command\n{command}\nfailed\nStdout: {stdout}\nStderr: {stderr}").into(),
        );
    }

    Ok(format!(
        "command\n{command}\nsuccess\nStdout: {stdout}\nStderr: {stderr}"
    ))
}
