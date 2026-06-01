use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{debug, warn};

use crate::agent_type::runtime_config::on_host::package::rendered::Postdownload;

#[derive(thiserror::Error, Debug)]
pub enum PostdownloadExecutionError {
    #[error("Postdownload command not found: {0}")]
    CommandNotFound(PathBuf),

    #[error("Postdownload script not found: {0}")]
    ScriptNotFound(PathBuf),

    #[error("Postdownload script execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Postdownload script timed out after {0:?}")]
    Timeout(Duration),
}

/// Check if a path is relative (supports both Unix and Windows separators)
fn is_relative_path(path: &str) -> bool {
    path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with(".\\")
        || path.starts_with("..\\")
}

/// Resolve a path relative to the given base directory
fn resolve_path(base_dir: &Path, path: &str) -> String {
    if is_relative_path(path) {
        base_dir.join(path).to_string_lossy().to_string()
    } else {
        path.to_string()
    }
}

/// Make a script executable (Unix only)
#[cfg(unix)]
fn make_script_executable(script_path: &Path) -> Result<(), PostdownloadExecutionError> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(script_path)
        .map_err(|e| {
            PostdownloadExecutionError::ExecutionFailed(format!(
                "Failed to read script permissions: {}",
                e
            ))
        })?
        .permissions();

    perms.set_mode(0o755);

    std::fs::set_permissions(script_path, perms).map_err(|e| {
        PostdownloadExecutionError::ExecutionFailed(format!(
            "Failed to make script executable: {}",
            e
        ))
    })
}

pub struct PostdownloadExecutor {
    package_dir: PathBuf,
}

impl PostdownloadExecutor {
    pub fn new(package_dir: PathBuf) -> Self {
        Self { package_dir }
    }

    pub fn execute(&self, postdownload: &Postdownload) -> Result<(), PostdownloadExecutionError> {
        if postdownload.args.len() < 2 {
            return Err(PostdownloadExecutionError::ExecutionFailed(
                "postdownload args must have at least 2 elements: command and script path"
                    .to_string(),
            ));
        }

        let command = &postdownload.args[0];
        let script_path = &postdownload.args[1];
        let script_args = &postdownload.args[2..];

        debug!(
            command = %command,
            script = %script_path,
            args = ?script_args,
            timeout = ?postdownload.timeout,
            "Executing postdownload"
        );

        // Postdownload structure is always: [command, script, script_args...]
        // - args[0] (command): The executor (bash, python3, ./bin/custom-tool, etc.)
        // - args[1] (script): The script to execute - validate existence + make executable on Unix
        // - args[2..] (script_args): Arguments for the script - validate existence only, no chmod

        if is_relative_path(command) {
            let command_path = self.package_dir.join(command);
            if !command_path.exists() {
                return Err(PostdownloadExecutionError::CommandNotFound(command_path));
            }
        }

        if is_relative_path(script_path) {
            let script_full_path = self.package_dir.join(script_path);

            if !script_full_path.exists() {
                return Err(PostdownloadExecutionError::ScriptNotFound(script_full_path));
            }

            // Make script executable (Unix only)
            #[cfg(unix)]
            make_script_executable(&script_full_path)?;
        }

        for script_arg in script_args {
            if is_relative_path(script_arg) {
                let arg_path = self.package_dir.join(script_arg);
                if !arg_path.exists() {
                    return Err(PostdownloadExecutionError::ScriptNotFound(arg_path));
                }
            }
        }

        let output = self.execute_with_timeout(postdownload)?;

        if output.status.success() {
            debug!(
                command = %command,
                "Postdownload completed successfully"
            );
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let error_msg = format!(
                "Postdownload failed with exit code {:?}\nstderr: {}",
                output.status.code(),
                stderr
            );
            warn!(command = %command, error = %error_msg);
            Err(PostdownloadExecutionError::ExecutionFailed(error_msg))
        }
    }

    fn execute_with_timeout(
        &self,
        postdownload: &Postdownload,
    ) -> Result<std::process::Output, PostdownloadExecutionError> {
        let package_dir = self.package_dir.clone();
        let env = postdownload.env.clone();
        let timeout = postdownload.timeout;

        let command = resolve_path(&package_dir, &postdownload.args[0]);
        let args: Vec<String> = postdownload.args[1..]
            .iter()
            .map(|arg| resolve_path(&package_dir, arg))
            .collect();

        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let mut cmd = Command::new(&command);

            cmd.args(&args);
            cmd.current_dir(&package_dir);
            cmd.env("PACKAGE_DIR", &package_dir);
            cmd.stdout(Stdio::null()); // Suppress stdout, only capture stderr

            for (key, value) in env.iter() {
                cmd.env(key, value);
            }

            let result = cmd.output();
            let _ = tx.send(result);
        });

        match rx.recv_timeout(timeout) {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(PostdownloadExecutionError::ExecutionFailed(format!(
                "Failed to execute command: {}",
                e
            ))),
            Err(_) => Err(PostdownloadExecutionError::Timeout(timeout)),
        }
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

    fn create_postdownload(args: Vec<String>) -> Postdownload {
        Postdownload {
            args,
            env: HashMap::new(),
            timeout: Duration::from_secs(5),
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
    fn test_execute_successful_postdownload() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_postdownload.sh");

        create_bash_script(&script_path, "echo 'Postdownload executed successfully'", 0);

        let postdownload = create_postdownload(vec![
            "bash".to_string(),
            "./test_postdownload.sh".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_failing_postdownload() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("failing_postdownload.sh");

        create_bash_script(&script_path, "echo 'Postdownload failed' >&2", 1);

        let postdownload = create_postdownload(vec![
            "bash".to_string(),
            "./failing_postdownload.sh".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::ExecutionFailed(_)
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_nonexistent_postdownload() {
        let temp_dir = TempDir::new().unwrap();

        let postdownload =
            create_postdownload(vec!["bash".to_string(), "./nonexistent.sh".to_string()]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::ScriptNotFound(_)
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_nonexistent_command() {
        let temp_dir = TempDir::new().unwrap();

        // Create a valid script but use a non-existent command
        let script_path = temp_dir.path().join("script.sh");
        create_bash_script(&script_path, "echo 'test'", 0);

        let postdownload = create_postdownload(vec![
            "./nonexistent-tool".to_string(),
            "./script.sh".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::CommandNotFound(_)
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

        // Script is in bin/ subdirectory, passed as argument
        let postdownload =
            create_postdownload(vec!["bash".to_string(), "./bin/my_script.sh".to_string()]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
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

        let postdownload = create_postdownload(vec![
            "bash".to_string(),
            "./install.sh".to_string(),
            "./config.yaml".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());

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

    // Windows tests
    #[test]
    #[cfg(windows)]
    fn test_execute_successful_postdownload_windows() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_postdownload.bat");

        create_batch_script(&script_path, "echo Postdownload executed successfully", 0);

        let postdownload = create_postdownload(vec![
            "cmd".to_string(),
            "/c".to_string(),
            "./test_postdownload.bat".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
    }

    #[test]
    #[cfg(windows)]
    fn test_execute_failing_postdownload_windows() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("failing_postdownload.bat");

        create_batch_script(&script_path, "echo Postdownload failed 1>&2", 1);

        let postdownload = create_postdownload(vec![
            "cmd".to_string(),
            "/c".to_string(),
            "./failing_postdownload.bat".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::ExecutionFailed(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_execute_nonexistent_postdownload_windows() {
        let temp_dir = TempDir::new().unwrap();

        let postdownload = create_postdownload(vec![
            "cmd".to_string(),
            "/c".to_string(),
            "./nonexistent.bat".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::ScriptNotFound(_)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn test_execute_nonexistent_command_windows() {
        let temp_dir = TempDir::new().unwrap();

        // Create a valid script but use a non-existent command
        let script_path = temp_dir.path().join("script.bat");
        create_batch_script(&script_path, "echo test", 0);

        let postdownload = create_postdownload(vec![
            ".\\nonexistent-tool.exe".to_string(), // Command doesn't exist
            ".\\script.bat".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        let result = executor.execute(&postdownload);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PostdownloadExecutionError::CommandNotFound(_)
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

        // Script is in bin\ subdirectory, passed as argument
        let postdownload = create_postdownload(vec![
            "cmd".to_string(),
            "/c".to_string(),
            ".\\bin\\my_script.bat".to_string(),
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
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

        let postdownload = create_postdownload(vec![
            "cmd".to_string(),
            "/c".to_string(),
            ".\\install.bat".to_string(),
            ".\\config.yaml".to_string(), // Config file argument
        ]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());

        // Both files should exist
        assert!(script_path.exists());
        assert!(config_path.exists());
    }
}
