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
        .expect("failed to execute Get-CimInstance");

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()))
        .map(|s| s.to_string())
        .collect()
}

/// Kills the processes with the given PIDs (and, on Windows, their descendants), ignoring
/// processes that are already gone.
pub fn kill_processes(pids: &[String]) {
    for pid in pids {
        let _ = cfg_select! {
            target_family = "unix" => Command::new("kill").args(["-KILL", pid]),
            target_family = "windows" => Command::new("taskkill").args(["/F", "/T", "/PID", pid])
        }
        .output();
    }
}

/// Kills the processes matching `pattern` when dropped, so a test can't leak them on failure.
pub struct KillProcessesOnDrop(pub String);

impl Drop for KillProcessesOnDrop {
    fn drop(&mut self) {
        kill_processes(&find_processes_by_pattern(&self.0));
    }
}
