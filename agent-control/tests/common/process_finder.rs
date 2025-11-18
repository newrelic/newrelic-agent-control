use std::process::Command;

/// Helper function to find processes by pattern (cross-platform)
#[cfg(target_family = "unix")]
pub fn find_processes_by_pattern(pattern: &str) -> Vec<String> {
    let output = Command::new("pgrep")
        .arg("-f")
        .arg(pattern)
        .output()
        .expect("failed to execute pgrep");

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(target_family = "windows")]
pub fn find_processes_by_pattern(pattern: &str) -> Vec<String> {
    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        // A sort of `pgrep` that searches for processes by command line pattern,
        // excluding the search command itself
        .arg(format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{}*' -and $_.CommandLine -notlike '*Get-CimInstance*' }} | Select-Object -ExpandProperty ProcessId",
            pattern
        ))
        .output()
        .expect("failed to execute Get-Process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()))
        .map(|s| s.to_string())
        .collect()
}
