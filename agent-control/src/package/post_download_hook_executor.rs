//! Executes a package's post-download hook command after extraction, enforcing a timeout.
use std::io::{Error as IoError, ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::agent_type::runtime_config::on_host::package::rendered::PostDownloadHook;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Errors that can occur while executing a post-download hook.
#[derive(thiserror::Error, Debug)]
pub enum PostDownloadHookExecutionError {
    /// The configured hook command could not be found.
    #[error("command not found: {path}")]
    CommandNotFound {
        /// Path of the command that could not be found.
        path: String,
    },

    /// The hook command could not be spawned.
    #[error("failed to spawn command '{0}': {1}")]
    SpawnFailed(String, #[source] IoError),

    /// The hook ran but exited with a non-zero status.
    #[error("script execution failed with exit code {0:?}\nstderr: {1}")]
    ExecutionFailed(Option<i32>, String),

    /// The hook did not complete within the allotted time.
    #[error("post-download hook timed out after {0:?}")]
    Timeout(Duration),
}

/// Reads an optional child output stream (stdout/stderr) to completion.
fn capture_output(stream: Option<impl Read>) -> String {
    let mut buf = Vec::new();
    if let Some(mut reader) = stream {
        let _ = reader.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Runs a package's post-download hook within its installation directory.
pub struct PostDownloadHookExecutor {
    package_dir: PathBuf,
    timeout: Duration,
}

impl PostDownloadHookExecutor {
    /// Creates an executor that runs hooks inside `package_dir`.
    pub fn new(package_dir: PathBuf) -> Self {
        Self {
            package_dir,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Executes the given post-download hook, returning an error on failure or timeout.
    pub fn execute(
        &self,
        post_download_hook: &PostDownloadHook,
    ) -> Result<(), PostDownloadHookExecutionError> {
        debug!(
            path = %post_download_hook.path,
            args = ?post_download_hook.args,
            "Executing post-download hook"
        );

        let mut cmd = Command::new(&post_download_hook.path);
        cmd.args(&post_download_hook.args.0)
            .current_dir(&self.package_dir)
            .env("PACKAGE_DIR", &self.package_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(&post_download_hook.env.0);

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                PostDownloadHookExecutionError::CommandNotFound {
                    path: post_download_hook.path.clone(),
                }
            } else {
                PostDownloadHookExecutionError::SpawnFailed(post_download_hook.path.clone(), e)
            }
        })?;

        // Drain stdout/stderr on dedicated threads avoiding deadlocks due to full pipe buffer.
        let stdout_handle = child.stdout.take();
        let stdout_reader = thread::spawn(move || capture_output(stdout_handle));
        let stderr_handle = child.stderr.take();
        let stderr_reader = thread::spawn(move || capture_output(stderr_handle));

        // Wait for completion with timeout
        let deadline = Instant::now() + self.timeout;

        loop {
            match child
                .try_wait()
                .expect("failed to check process status - internal OS error")
            {
                Some(status) => {
                    let stdout = stdout_reader.join().unwrap_or_default();
                    let stderr = stderr_reader.join().unwrap_or_default();

                    if status.success() {
                        debug!(
                            path = %post_download_hook.path,
                            stdout = %stdout,
                            stderr = %stderr,
                            "Post-download hook completed successfully"
                        );
                        return Ok(());
                    } else {
                        warn!(
                            path = %post_download_hook.path,
                            exit_code = ?status.code(),
                            stdout = %stdout,
                            stderr = %stderr,
                            "Post-download hook execution failed"
                        );
                        return Err(PostDownloadHookExecutionError::ExecutionFailed(
                            status.code(),
                            stderr,
                        ));
                    }
                }
                None => {
                    if Instant::now() >= deadline {
                        // TODO improve this to close the process tree
                        let _ = child.kill();
                        let _ = child.wait();
                        let stdout = stdout_reader.join().unwrap_or_default();
                        let stderr = stderr_reader.join().unwrap_or_default();
                        warn!(
                            path = %post_download_hook.path,
                            stdout = %stdout,
                            stderr = %stderr,
                            "Post-download hook timed out"
                        );
                        return Err(PostDownloadHookExecutionError::Timeout(self.timeout));
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs::{File, create_dir};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use tracing_test::traced_test;

    use crate::agent_type::runtime_config::on_host::executable::rendered::{Args, Env};

    fn create_post_download_hook(path: String, args: Vec<String>) -> PostDownloadHook {
        PostDownloadHook {
            path,
            args: Args(args),
            env: Env(HashMap::new()),
        }
    }

    /// Creates a test script with platform-specific format
    fn create_script(path: &Path, content: &str, exit_code: i32) {
        let mut file = File::create(path).unwrap();

        #[cfg(unix)]
        {
            writeln!(file, "#!/bin/bash").unwrap();
            writeln!(file, "{}", content).unwrap();
            writeln!(file, "exit {}", exit_code).unwrap();
        }

        #[cfg(windows)]
        {
            writeln!(file, "@echo off").unwrap();
            writeln!(file, "{}", content).unwrap();
            writeln!(file, "exit /b {}", exit_code).unwrap();
        }
    }

    /// Returns a script command that writes `text` to stdout without a trailing newline.
    /// Avoiding the trailing newline keeps a field's value on a single log line, which the
    /// line-scoped `logs_contain` assertions rely on.
    fn print_stdout_no_newline(text: &str) -> String {
        #[cfg(unix)]
        return format!("printf '{text}'");

        #[cfg(windows)]
        return format!("<nul set /p x={text}");
    }

    /// Returns a script command that writes `text` to stderr without a trailing newline.
    fn print_stderr_no_newline(text: &str) -> String {
        #[cfg(unix)]
        return format!("printf '{text}' >&2");

        #[cfg(windows)]
        return format!("<nul set /p x={text} 1>&2");
    }

    /// Returns a script command that blocks for approximately 5 seconds.
    fn sleep_command() -> String {
        #[cfg(unix)]
        return "sleep 5".to_string();

        #[cfg(windows)]
        return "ping -n 6 127.0.0.1 >nul".to_string();
    }

    /// Joins script commands into a single line using the platform's statement separator
    fn join_commands(commands: &[String]) -> String {
        #[cfg(unix)]
        let separator = "; ";
        #[cfg(windows)]
        let separator = " & ";

        commands.join(separator)
    }

    /// Returns the script file extension for the current platform
    fn script_extension() -> &'static str {
        #[cfg(unix)]
        return "sh";

        #[cfg(windows)]
        return "bat";
    }

    /// Returns the shell command and required args for executing scripts on the current platform
    fn get_shell_command() -> (String, Vec<String>) {
        #[cfg(unix)]
        {
            ("bash".to_string(), vec![])
        }

        #[cfg(windows)]
        {
            let cmd = std::env::var("COMSPEC")
                .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string());
            (cmd, vec!["/c".to_string()])
        }
    }

    /// Creates a PostDownloadHook that executes a script with the appropriate shell
    fn create_script_hook(script_path: PathBuf, additional_args: Vec<String>) -> PostDownloadHook {
        let (shell_cmd, mut shell_args) = get_shell_command();
        shell_args.push(script_path.to_string_lossy().to_string());
        shell_args.extend(additional_args);

        create_post_download_hook(shell_cmd, shell_args)
    }

    /// Sets up a test with temp directory, script path, and executor
    /// Returns (temp_dir, script_path, executor)
    fn setup_test_script(script_name: &str) -> (TempDir, PathBuf, PostDownloadHookExecutor) {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir
            .path()
            .join(format!("{}.{}", script_name, script_extension()));
        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        (temp_dir, script_path, executor)
    }

    #[test]
    fn test_execute_successful_post_download_hook() {
        let (_temp_dir, script_path, executor) = setup_test_script("test_post_download_hook");

        create_script(
            &script_path,
            "echo 'Post-download hook executed successfully'",
            0,
        );

        let post_download_hook = create_script_hook(script_path, vec![]);
        let result = executor.execute(&post_download_hook);
        if let Err(e) = &result {
            eprintln!("Error: {}", e);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_failing_post_download_hook() {
        let (_temp_dir, script_path, executor) = setup_test_script("failing_post_download_hook");

        create_script(&script_path, "echo 'Post-download hook failed' >&2", 1);

        let post_download_hook = create_script_hook(script_path, vec![]);
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostDownloadHookExecutionError::ExecutionFailed { .. }
        ));
    }

    #[traced_test]
    #[test]
    fn test_execute_successful_post_download_hook_logs_stdout_and_stderr() {
        let (_temp_dir, script_path, executor) =
            setup_test_script("successful_hook_output_logging");

        let content = join_commands(&[
            print_stdout_no_newline("hook-stdout-line"),
            print_stderr_no_newline("hook-stderr-line"),
        ]);
        create_script(&script_path, &content, 0);

        let post_download_hook = create_script_hook(script_path, vec![]);
        let result = executor.execute(&post_download_hook);

        assert!(result.is_ok());
        assert!(logs_contain("Post-download hook completed successfully"));
        assert!(logs_contain("stdout=hook-stdout-line"));
        assert!(logs_contain("stderr=hook-stderr-line"));
    }

    #[traced_test]
    #[test]
    fn test_execute_failing_post_download_hook_logs_stdout_and_stderr() {
        let (_temp_dir, script_path, executor) = setup_test_script("failing_hook_output_logging");

        let content = join_commands(&[
            print_stdout_no_newline("about-to-fail"),
            print_stderr_no_newline("hook-stderr-line"),
        ]);
        create_script(&script_path, &content, 1);

        let post_download_hook = create_script_hook(script_path, vec![]);
        let result = executor.execute(&post_download_hook);

        match result.unwrap_err() {
            PostDownloadHookExecutionError::ExecutionFailed(exit_code, stderr) => {
                assert_eq!(exit_code, Some(1));
                assert!(stderr.contains("hook-stderr-line"));
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
        assert!(logs_contain("Post-download hook execution failed"));
        assert!(logs_contain("stdout=about-to-fail"));
        assert!(logs_contain("stderr=hook-stderr-line"));
    }

    #[traced_test]
    #[test]
    fn test_execute_timeout_logs_partial_output() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir
            .path()
            .join(format!("hanging_hook.{}", script_extension()));

        let content = join_commands(&[
            print_stdout_no_newline("partial-stdout"),
            print_stderr_no_newline("partial-stderr"),
            sleep_command(),
        ]);
        create_script(&script_path, &content, 0);

        let post_download_hook = create_script_hook(script_path, vec![]);
        let executor = PostDownloadHookExecutor {
            package_dir: temp_dir.path().to_path_buf(),
            timeout: Duration::from_millis(200),
        };

        let result = executor.execute(&post_download_hook);

        assert!(matches!(
            result.unwrap_err(),
            PostDownloadHookExecutionError::Timeout(_)
        ));
        assert!(logs_contain("Post-download hook timed out"));
        assert!(logs_contain("stdout=partial-stdout"));
        assert!(logs_contain("stderr=partial-stderr"));
    }

    #[test]
    fn test_execute_script_in_subdirectory() {
        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        create_dir(&bin_dir).unwrap();

        let script_path = bin_dir.join(format!("my_script.{}", script_extension()));
        create_script(&script_path, "echo 'Script executed from subdirectory'", 0);

        let post_download_hook = create_script_hook(script_path, vec![]);

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());
    }

    #[test]
    fn test_script_with_config_file_argument() {
        let (temp_dir, script_path, executor) = setup_test_script("install");

        // Create script that reads the config file passed as argument
        #[cfg(unix)]
        let script_content = "cat $1";

        #[cfg(windows)]
        let script_content = "type %1";

        create_script(&script_path, script_content, 0);

        // Create config file
        let config_path = temp_dir.path().join("config.yaml");
        let mut config_file = File::create(&config_path).unwrap();
        writeln!(config_file, "setting: value").unwrap();

        let post_download_hook =
            create_script_hook(script_path, vec![config_path.to_string_lossy().to_string()]);

        assert!(executor.execute(&post_download_hook).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_binary_without_args() {
        let (_temp_dir, _script_path, executor) = setup_test_script("unused");

        // Use a simple binary that doesn't require arguments (true always succeeds)
        let post_download_hook = create_post_download_hook("/usr/bin/true".to_string(), vec![]);

        let result = executor.execute(&post_download_hook);

        // Should succeed - args can be empty for binaries that don't need arguments
        assert!(
            result.is_ok(),
            "Binary execution without args should work: {:?}",
            result
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_with_command_in_path() {
        let (_temp_dir, script_path, executor) = setup_test_script("test_script");

        create_script(&script_path, "echo 'Using command from PATH'", 0);

        // Use "bash" instead of "/bin/bash" - should find it in PATH
        let post_download_hook = create_post_download_hook(
            "bash".to_string(),
            vec![script_path.to_string_lossy().to_string()],
        );

        let result = executor.execute(&post_download_hook);

        // Should succeed - "bash" is found in PATH
        assert!(
            result.is_ok(),
            "Command from PATH should work: {:?}",
            result
        );
    }
}
