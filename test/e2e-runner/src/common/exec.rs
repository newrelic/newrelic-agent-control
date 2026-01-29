use std::io::{BufRead, BufReader};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::common::test::TestResult;

pub struct LongRunningProcess {
    child: std::process::Child,
    output_buffer: Arc<Mutex<String>>,
}

impl LongRunningProcess {
    /// Spawns a long-running process using the provided command.
    /// Continuously captures stdout and stderr in the background.
    pub fn spawn(cmd: Command) -> Self {
        let mut cmd = cmd;
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("spawning process: {}", e));

        let output_buffer = Arc::new(Mutex::new(String::new()));

        // Spawn thread to read stdout
        if let Some(stdout) = child.stdout.take() {
            let buffer = Arc::clone(&output_buffer);
            let _ = thread::Builder::new()
                .name("stdout-reader".to_string())
                .spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        let Ok(line) = line else {
                            continue;
                        };
                        if let Ok(mut buf) = buffer.lock() {
                            buf.push_str(&line);
                            buf.push('\n');
                        }
                    }
                });
        }

        // Spawn thread to read stderr
        if let Some(stderr) = child.stderr.take() {
            let buffer = Arc::clone(&output_buffer);
            let _ = thread::Builder::new()
                .name("stderr-reader".to_string())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        let Ok(line) = line else {
                            continue;
                        };
                        if let Ok(mut buf) = buffer.lock() {
                            buf.push_str(&line);
                            buf.push('\n');
                        }
                    }
                });
        }

        Self {
            child,
            output_buffer,
        }
    }

    /// Returns all stdout and stderr output captured so far without blocking.
    /// Does not kill or interfere with the running process.
    pub fn current_output(&self) -> String {
        self.output_buffer
            .lock()
            .unwrap_or_else(|e| panic!("failed to lock output buffer: {}", e))
            .clone()
    }
}
impl Drop for LongRunningProcess {
    fn drop(&mut self) {
        #[cfg(target_family = "windows")]
        {
            let mut cmd = Command::new("taskkill");
            // kills the process and all its child processes
            cmd.args(["/PID", &self.child.id().to_string(), "/T", "/F"]);
            let _ = cmd.output();
        }
        #[cfg(target_family = "unix")]
        {
            let _ = self.child.kill();
        }
    }
}

/// Executes the provided [Command] and resturs its output or an error.
pub fn exec_cmd(cmd: &mut Command) -> TestResult<String> {
    let output = cmd.output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(format!("command failed\nStdout: {stdout}\nStderr: {stderr}").into());
    }

    Ok(format!(
        "command\n{cmd:?}\nsuccess\nStdout: {stdout}\nStderr: {stderr}"
    ))
}
