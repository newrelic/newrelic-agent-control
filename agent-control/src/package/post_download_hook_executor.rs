use std::io::{Error as IoError, ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::agent_type::runtime_config::on_host::package::rendered::PostDownloadHook;

#[cfg(unix)]
use {
    std::fs::{metadata, set_permissions},
    std::os::unix::fs::PermissionsExt,
    std::path::Path,
};

#[derive(thiserror::Error, Debug)]
pub enum PostDownloadHookExecutionError {
    #[error("command not found: {path}")]
    CommandNotFound { path: String },

    #[error("failed to spawn command '{0}': {1}")]
    SpawnFailed(String, #[source] IoError),

    #[error("script execution failed with exit code {0:?}\nstderr: {1}")]
    ExecutionFailed(Option<i32>, String),

    #[error("post-download hook timed out after {0:?}")]
    Timeout(Duration),
}

#[cfg(unix)]
fn make_executable_if_exists(path: &str) {
    let file_path = Path::new(path);
    if file_path.is_file()
        && let Ok(meta) = metadata(file_path)
    {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = set_permissions(file_path, perms);
    }
}

pub struct PostDownloadHookExecutor {
    package_dir: PathBuf,
}

impl PostDownloadHookExecutor {
    pub fn new(package_dir: PathBuf) -> Self {
        Self { package_dir }
    }

    pub fn execute(
        &self,
        post_download_hook: &PostDownloadHook,
    ) -> Result<(), PostDownloadHookExecutionError> {
        debug!(
            path = %post_download_hook.path,
            args = ?post_download_hook.args,
            "Executing post-download hook"
        );

        #[cfg(unix)]
        make_executable_if_exists(&post_download_hook.path);

        let mut cmd = Command::new(&post_download_hook.path);
        cmd.args(&post_download_hook.args.0)
            .current_dir(&self.package_dir)
            .env("PACKAGE_DIR", &self.package_dir)
            .stdout(Stdio::null())
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

        // Wait for completion with timeout
        let timeout = Duration::from_secs(300);
        let deadline = Instant::now() + timeout;
        const POLL_INTERVAL: Duration = Duration::from_millis(100);

        loop {
            match child
                .try_wait()
                .expect("failed to check process status - internal OS error")
            {
                Some(status) => {
                    let stderr = child
                        .stderr
                        .take()
                        .and_then(|mut stderr| {
                            let mut buf = Vec::new();
                            stderr.read_to_end(&mut buf).ok().map(|_| buf)
                        })
                        .unwrap_or_default();

                    if status.success() {
                        debug!(
                            path = %post_download_hook.path,
                            "Post-download hook completed successfully"
                        );
                        return Ok(());
                    } else {
                        let stderr_str = String::from_utf8_lossy(&stderr).to_string();
                        warn!(
                            path = %post_download_hook.path,
                            exit_code = ?status.code(),
                            stderr = %stderr_str,
                            "Post-download hook execution failed"
                        );
                        return Err(PostDownloadHookExecutionError::ExecutionFailed(
                            status.code(),
                            stderr_str,
                        ));
                    }
                }
                None => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(PostDownloadHookExecutionError::Timeout(timeout));
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
    fn test_direct_script_execution_without_execute_permission() {
        let (_temp_dir, script_path, executor) = setup_test_script("direct_script");

        create_script(&script_path, "echo 'Direct execution works'", 0);

        // Explicitly remove execute permissions
        let mut perms = metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o644);
        set_permissions(&script_path, perms).unwrap();

        // Verify script is NOT executable before test
        let perms_before = metadata(&script_path).unwrap().permissions();
        assert_eq!(
            perms_before.mode() & 0o111,
            0,
            "Script should not be executable initially"
        );

        // Execute script directly (path points to script, not interpreter)
        let post_download_hook = create_post_download_hook(
            script_path.to_string_lossy().to_string(),
            vec![script_path.to_string_lossy().to_string()],
        );

        let result = executor.execute(&post_download_hook);

        // Should succeed because make_executable_if_exists() makes it executable
        assert!(
            result.is_ok(),
            "Direct script execution should work after auto-chmod"
        );

        // Verify script is now executable
        let perms_after = metadata(&script_path).unwrap().permissions();
        assert_eq!(
            perms_after.mode() & 0o111,
            0o111,
            "Script should be executable after execution"
        );
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
