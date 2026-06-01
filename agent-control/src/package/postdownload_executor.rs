use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use crate::agent_type::runtime_config::on_host::package::rendered::Postdownload;

#[derive(thiserror::Error, Debug)]
pub enum PostdownloadExecutionError {
    #[error("Postdownload script not found: {0}")]
    ScriptNotFound(PathBuf),

    #[error("Postdownload script execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Postdownload script timed out after {0:?}")]
    Timeout(Duration),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
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

pub struct PostdownloadExecutor {
    /// Base directory where the package is located (for relative script paths)
    package_dir: PathBuf,
}

impl PostdownloadExecutor {
    pub fn new(package_dir: PathBuf) -> Self {
        Self { package_dir }
    }

    /// Execute a postdownload script with timeout
    pub fn execute(&self, postdownload: &Postdownload) -> Result<(), PostdownloadExecutionError> {
        if postdownload.args.is_empty() {
            return Err(PostdownloadExecutionError::ExecutionFailed(
                "postdownload args cannot be empty".to_string(),
            ));
        }

        let command = &postdownload.args[0];
        let args = &postdownload.args[1..];

        info!(
            command = %command,
            args = ?args,
            timeout = ?postdownload.timeout,
            "Executing postdownload"
        );

        let resolve_and_make_executable = |path: &str| -> Result<(), PostdownloadExecutionError> {
            let resolved_path = self.package_dir.join(path);

            if !resolved_path.exists() {
                return Err(PostdownloadExecutionError::ScriptNotFound(resolved_path));
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&resolved_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&resolved_path, perms)?;
            }

            Ok(())
        };

        if is_relative_path(command) {
            resolve_and_make_executable(command)?;
        }

        for arg in args {
            if is_relative_path(arg) {
                resolve_and_make_executable(arg)?;
            }
        }

        let output = self.execute_with_timeout(postdownload)?;

        if output.status.success() {
            info!(
                command = %command,
                "Postdownload completed successfully"
            );
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let error_msg = format!(
                "Postdownload failed with exit code {:?}\nstdout: {}\nstderr: {}",
                output.status.code(),
                stdout,
                stderr
            );
            warn!(command = %command, error = %error_msg);
            Err(PostdownloadExecutionError::ExecutionFailed(error_msg))
        }
    }

    /// Execute a command with timeout
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

            for (key, value) in env.iter() {
                cmd.env(key, value);
            }

            let result = cmd.output();
            let _ = tx.send(result);
        });

        match rx.recv_timeout(timeout) {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(PostdownloadExecutionError::IoError(e)),
            Err(_) => Err(PostdownloadExecutionError::Timeout(timeout)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_postdownload(args: Vec<String>) -> Postdownload {
        Postdownload {
            args,
            env: std::collections::HashMap::new(),
            timeout: Duration::from_secs(5),
        }
    }

    #[cfg(unix)]
    fn create_bash_script(path: &std::path::Path, content: &str, exit_code: i32) {
        let mut file = std::fs::File::create(path).unwrap();
        writeln!(file, "#!/bin/bash").unwrap();
        writeln!(file, "{}", content).unwrap();
        writeln!(file, "exit {}", exit_code).unwrap();
    }

    #[cfg(windows)]
    fn create_batch_script(path: &std::path::Path, content: &str, exit_code: i32) {
        let mut file = std::fs::File::create(path).unwrap();
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
    fn test_execute_with_relative_command_path() {
        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();

        let script_path = bin_dir.join("my_script.sh");
        create_bash_script(&script_path, "echo 'Script executed from relative path'", 0);

        let postdownload = create_postdownload(vec!["./bin/my_script.sh".to_string()]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
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
    fn test_execute_with_relative_command_path_windows() {
        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();

        let script_path = bin_dir.join("my_script.bat");
        create_batch_script(&script_path, "echo Script executed from relative path", 0);

        let postdownload = create_postdownload(vec![".\\bin\\my_script.bat".to_string()]);

        let executor = PostdownloadExecutor::new(temp_dir.path().to_path_buf());
        assert!(executor.execute(&postdownload).is_ok());
    }
}
