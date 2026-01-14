use std::process::Command;
/// Check if there is any process running that matches the given command line pattern
pub fn is_process_running(cmd_line_pattern: &str) -> bool {
    // Check for processes matching the pattern excluding the current process
    let ps = format!(
        r#"gwmi Win32_Process | ? {{ ($_.CommandLine -match "{cmd_line_pattern}") -and ($_.ProcessId -ne $PID) }} | select ProcessId, CommandLine"#
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .unwrap();

    println!("{:?}", output);

    !output.stdout.is_empty()
}

/// Check if there is any orphan process running that matches the given command line pattern
pub fn is_process_orphan(cmd_line_pattern: &str) -> bool {
    // Count processes matching the pattern whose ParentProcessId is not present among running PIDs
    let ps = format!(
        r#"$p = gwmi Win32_Process; $p | ? {{ ($_.CommandLine -match "{cmd_line_pattern}") -and ($p.ProcessId -notcontains $_.ParentProcessId) }} | select ProcessId, ParentProcessId, CommandLine"#
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .unwrap();

    println!("{:?}", output);

    !output.stdout.is_empty()
}
