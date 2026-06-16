use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::agent_type::runtime_config::on_host::package::rendered::PostDownloadHook;
use crate::utils::thread_context::NotStartedThreadContext;

#[derive(thiserror::Error, Debug)]
pub enum PostDownloadHookExecutionError {
    #[error("post_download_hook args cannot be empty, must contain at least the script path")]
    EmptyArgs,

    #[error("path must be absolute: {path}")]
    PathNotAbsolute { path: String },

    #[error("script path (first argument) must be absolute: {path}")]
    ScriptPathNotAbsolute { path: String },

    #[error("command not found: {path}")]
    CommandNotFound { path: String },

    #[error("script not found: {path}")]
    ScriptNotFound { path: String },

    #[error("failed to set script permissions: {0}")]
    ScriptPermissionError(#[source] std::io::Error),

    #[error("failed to spawn command '{0}': {1}")]
    SpawnFailed(String, #[source] std::io::Error),

    #[error("script execution failed with exit code {0:?}\nstderr: {1}")]
    ExecutionFailed(Option<i32>, String),

    #[error("post-download hook timed out after {0:?}")]
    Timeout(Duration),
}

#[cfg(unix)]
fn make_script_executable(script_path: &Path) -> Result<(), PostDownloadHookExecutionError> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(script_path)
        .map_err(PostDownloadHookExecutionError::ScriptPermissionError)?
        .permissions();

    perms.set_mode(0o755);

    std::fs::set_permissions(script_path, perms)
        .map_err(PostDownloadHookExecutionError::ScriptPermissionError)
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
        if post_download_hook.args.is_empty() {
            return Err(PostDownloadHookExecutionError::EmptyArgs);
        }

        // Validate that path is absolute
        let path = Path::new(&post_download_hook.path);
        if !path.is_absolute() {
            return Err(PostDownloadHookExecutionError::PathNotAbsolute {
                path: post_download_hook.path.clone(),
            });
        }

        // Validate that script path (args[0]) is absolute
        let script_path = Path::new(&post_download_hook.args[0]);
        if !script_path.is_absolute() {
            return Err(PostDownloadHookExecutionError::ScriptPathNotAbsolute {
                path: post_download_hook.args[0].clone(),
            });
        }

        let script_args = &post_download_hook.args[1..];

        debug!(
            path = %post_download_hook.path,
            script = %post_download_hook.args[0],
            args = ?script_args,
            "Executing post-download hook"
        );

        // Make script executable on Unix if it exists
        if script_path.exists() {
            #[cfg(unix)]
            make_script_executable(script_path)?;
        }

        let output = self.execute_with_timeout(post_download_hook)?;

        if output.status.success() {
            debug!(
                path = %post_download_hook.path,
                "Post-download hook completed successfully"
            );
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            warn!(
                path = %post_download_hook.path,
                exit_code = ?output.status.code(),
                stderr = %stderr,
                "Post-download hook execution failed"
            );
            Err(PostDownloadHookExecutionError::ExecutionFailed(
                output.status.code(),
                stderr,
            ))
        }
    }

    fn execute_with_timeout(
        &self,
        post_download_hook: &PostDownloadHook,
    ) -> Result<std::process::Output, PostDownloadHookExecutionError> {
        let package_dir = self.package_dir.clone();
        let env = post_download_hook.env.clone();

        let timeout = Duration::from_secs(300);

        let path = post_download_hook.path.clone();
        let args = post_download_hook.args.clone();

        let thread_context =
            NotStartedThreadContext::new("post-download-hook", move |_stop_consumer| {
                let mut cmd = Command::new(&path);

                cmd.args(&args);
                cmd.current_dir(&package_dir);
                cmd.env("PACKAGE_DIR", &package_dir);
                cmd.stdout(Stdio::null());
                cmd.stderr(Stdio::piped());
                cmd.envs(&env);

                let mut child = match cmd.spawn() {
                    Ok(child) => child,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            return Err(PostDownloadHookExecutionError::CommandNotFound {
                                path: path.clone(),
                            });
                        }
                        return Err(PostDownloadHookExecutionError::SpawnFailed(path.clone(), e));
                    }
                };

                let deadline = Instant::now() + timeout;
                const POLL_INTERVAL: Duration = Duration::from_millis(100);

                loop {
                    match child
                        .try_wait()
                        .expect("failed to check process status - internal OS error")
                    {
                        Some(status) => {
                            let output = std::process::Output {
                                status,
                                stdout: Vec::new(),
                                stderr: child
                                    .stderr
                                    .take()
                                    .and_then(|mut stderr| {
                                        use std::io::Read;
                                        let mut buf = Vec::new();
                                        stderr.read_to_end(&mut buf).ok().map(|_| buf)
                                    })
                                    .unwrap_or_default(),
                            };
                            return Ok(output);
                        }
                        None => {
                            if Instant::now() >= deadline {
                                let _ = child.kill();
                                let _ = child.wait();
                                return Err(PostDownloadHookExecutionError::Timeout(timeout));
                            }
                            std::thread::sleep(POLL_INTERVAL);
                        }
                    }
                }
            })
            .start();

        thread_context.stop_blocking().expect(
            "post-download hook thread unexpectedly failed - this is an internal bug, \
             all user errors should be caught inside the thread",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs::{File, create_dir};
    use std::io::Write;
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::fs::{metadata, set_permissions};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn create_post_download_hook(path: String, args: Vec<String>) -> PostDownloadHook {
        PostDownloadHook {
            path,
            args,
            env: HashMap::new(),
        }
    }

    #[cfg(unix)]
    fn create_bash_script(path: &Path, content: &str, exit_code: i32) {
        let mut file = File::create(path).unwrap();
        writeln!(file, "#!/bin/bash").unwrap();
        writeln!(file, "{}", content).unwrap();
        writeln!(file, "exit {}", exit_code).unwrap();
    }

    #[cfg(windows)]
    fn create_batch_script(path: &std::path::Path, content: &str, exit_code: i32) {
        let mut file = File::create(path).unwrap();
        writeln!(file, "@echo off").unwrap();
        writeln!(file, "{}", content).unwrap();
        writeln!(file, "exit /b {}", exit_code).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_successful_post_download_hook() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_post_download_hook.sh");

        create_bash_script(
            &script_path,
            "echo 'Post-download hook executed successfully'",
            0,
        );

        // Canonicalize to get absolute path
        let absolute_script_path = script_path.canonicalize().unwrap();

        let post_download_hook = create_post_download_hook(
            "/bin/bash".to_string(),
            vec![absolute_script_path.to_string_lossy().to_string()],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);
        if let Err(e) = &result {
            eprintln!("Error: {}", e);
        }
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_failing_post_download_hook() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("failing_post_download_hook.sh");

        create_bash_script(&script_path, "echo 'Post-download hook failed' >&2", 1);

        let absolute_script_path = script_path.canonicalize().unwrap();

        let post_download_hook = create_post_download_hook(
            "/bin/bash".to_string(),
            vec![absolute_script_path.to_string_lossy().to_string()],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostDownloadHookExecutionError::ExecutionFailed { .. }
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_script_in_subdirectory() {
        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        create_dir(&bin_dir).unwrap();

        let script_path = bin_dir.join("my_script.sh");
        create_bash_script(&script_path, "echo 'Script executed from subdirectory'", 0);

        let absolute_script_path = script_path.canonicalize().unwrap();

        let post_download_hook = create_post_download_hook(
            "/bin/bash".to_string(),
            vec![absolute_script_path.to_string_lossy().to_string()],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_script_with_config_file_argument() {
        let temp_dir = TempDir::new().unwrap();

        // Create script
        let script_path = temp_dir.path().join("install.sh");
        create_bash_script(&script_path, "cat $1", 0);

        // Create config file (NOT executable)
        let config_path = temp_dir.path().join("config.yaml");
        let mut config_file = File::create(&config_path).unwrap();
        writeln!(config_file, "setting: value").unwrap();

        // Set config as read-only (NOT executable)
        let mut perms = metadata(&config_path).unwrap().permissions();
        perms.set_mode(0o644);
        set_permissions(&config_path, perms).unwrap();

        let absolute_script_path = script_path.canonicalize().unwrap();
        let absolute_config_path = config_path.canonicalize().unwrap();

        let post_download_hook = create_post_download_hook(
            "/bin/bash".to_string(),
            vec![
                absolute_script_path.to_string_lossy().to_string(),
                absolute_config_path.to_string_lossy().to_string(),
            ],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());

        // Verify script is executable but config is NOT
        let script_perms = metadata(&script_path).unwrap().permissions();
        assert_eq!(
            script_perms.mode() & 0o111,
            0o111,
            "Script should be executable"
        );

        let config_perms = metadata(&config_path).unwrap().permissions();
        assert_eq!(
            config_perms.mode() & 0o111,
            0,
            "Config should NOT be executable"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_reject_relative_path() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("script.sh");
        create_bash_script(&script_path, "echo 'test'", 0);

        // Relative path for command should be rejected
        let post_download_hook = create_post_download_hook(
            "bash".to_string(),
            vec![script_path.to_string_lossy().to_string()],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            PostDownloadHookExecutionError::PathNotAbsolute { .. }
        ));
        assert!(err.to_string().contains("path must be absolute"));
    }

    #[test]
    #[cfg(unix)]
    fn test_reject_relative_script_path() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("script.sh");
        create_bash_script(&script_path, "echo 'test'", 0);

        // Relative script path should be rejected
        let post_download_hook =
            create_post_download_hook("/bin/bash".to_string(), vec!["./script.sh".to_string()]);

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            PostDownloadHookExecutionError::ScriptPathNotAbsolute { .. }
        ));
        assert!(err.to_string().contains("script path"));
        assert!(err.to_string().contains("must be absolute"));
    }

    // Windows tests
    #[test]
    #[cfg(windows)]
    fn test_execute_successful_post_download_hook_windows() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_post_download_hook.bat");

        create_batch_script(
            &script_path,
            "echo Post-download hook executed successfully",
            0,
        );

        let absolute_script_path = script_path.canonicalize().unwrap();
        let post_download_hook = create_post_download_hook(
            "cmd".to_string(),
            vec![
                "/c".to_string(),
                absolute_script_path.to_string_lossy().to_string(),
            ],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());
    }

    #[test]
    #[cfg(windows)]
    fn test_execute_failing_post_download_hook_windows() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("failing_post_download_hook.bat");

        create_batch_script(&script_path, "echo Post-download hook failed 1>&2", 1);

        let absolute_script_path = script_path.canonicalize().unwrap();
        let post_download_hook = create_post_download_hook(
            "cmd".to_string(),
            vec![
                "/c".to_string(),
                absolute_script_path.to_string_lossy().to_string(),
            ],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostDownloadHookExecutionError::ExecutionFailed { .. }
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_execute_script_in_subdirectory_windows() {
        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        create_dir(&bin_dir).unwrap();

        let script_path = bin_dir.join("my_script.bat");
        create_batch_script(&script_path, "echo Script executed from subdirectory", 0);

        let absolute_script_path = script_path.canonicalize().unwrap();
        let post_download_hook = create_post_download_hook(
            "cmd".to_string(),
            vec![
                "/c".to_string(),
                absolute_script_path.to_string_lossy().to_string(),
            ],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());
    }

    #[test]
    #[cfg(windows)]
    fn test_script_with_config_file_argument_windows() {
        let temp_dir = TempDir::new().unwrap();

        // Create script that reads config file
        let script_path = temp_dir.path().join("install.bat");
        create_batch_script(&script_path, "type %1", 0);

        // Create config file
        let config_path = temp_dir.path().join("config.yaml");
        let mut config_file = File::create(&config_path).unwrap();
        writeln!(config_file, "setting: value").unwrap();

        let absolute_script_path = script_path.canonicalize().unwrap();
        let absolute_config_path = config_path.canonicalize().unwrap();

        let post_download_hook = create_post_download_hook(
            "cmd".to_string(),
            vec![
                "/c".to_string(),
                absolute_script_path.to_string_lossy().to_string(),
                absolute_config_path.to_string_lossy().to_string(),
            ],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&post_download_hook).is_ok());

        // Both files should exist
        assert!(script_path.exists());
        assert!(config_path.exists());
    }

    #[test]
    #[cfg(windows)]
    fn test_reject_relative_script_path_windows() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("script.bat");
        create_batch_script(&script_path, "echo test", 0);

        // Relative script path should be rejected
        let post_download_hook = create_post_download_hook(
            "cmd".to_string(),
            vec!["/c".to_string(), ".\\script.bat".to_string()],
        );

        let executor = PostDownloadHookExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&post_download_hook);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            PostDownloadHookExecutionError::ScriptPathNotAbsolute { .. }
        ));
        assert!(err.to_string().contains("script path"));
        assert!(err.to_string().contains("must be absolute"));
    }
}
