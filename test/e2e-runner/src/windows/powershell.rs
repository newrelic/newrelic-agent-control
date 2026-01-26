use std::process::Command;

use crate::common::exec::exec_cmd;
use crate::common::test::TestResult;

/// Executes a PowerShell command and returns its output or an error.
pub fn exec_ps(command: impl AsRef<str>) -> TestResult<String> {
    let mut cmd = Command::new("powershell.exe");
    let cmd = cmd.arg("-Command").arg(command.as_ref());
    exec_cmd(cmd)
        .map_err(|err| format!("failed to execute command '{}': {}", command.as_ref(), err).into())
}

/// Extracts a compressed archive to the specified destination.
pub fn extract(compressed_file: impl AsRef<str>, destination: impl AsRef<str>) {
    let extract_cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        compressed_file.as_ref(),
        destination.as_ref()
    );
    exec_ps(&extract_cmd).unwrap_or_else(|err| panic!("Failed to extract archive: {}", err));
}
/// Downloads a file from the specified URL to the given destination.
pub fn download_file(url: impl AsRef<str>, destination: impl AsRef<str>) {
    if test_file_exists(destination.as_ref()).is_ok() {
        return;
    }
    exec_ps(format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        url.as_ref(),
        destination.as_ref()
    ))
    .unwrap_or_else(|err| panic!("Failed to download file from {}: {}", url.as_ref(), err));
}

pub fn test_file_exists(path: impl AsRef<str>) -> TestResult<()> {
    let check_cmd = format!("Test-Path '{}'", path.as_ref());
    let result = exec_ps(&check_cmd)?;
    if result.contains("True") {
        Ok(())
    } else {
        Err(format!("File '{}' does not exist", path.as_ref()).into())
    }
}
