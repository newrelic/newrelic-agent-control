use newrelic_agent_control::agent_control::run::BasePaths;
use std::path::PathBuf;
use tempfile::{TempDir, tempdir};

/// Owns three temporary directories for a test and exposes a ready-to-use [`BasePaths`].
///
/// The directories are removed when this struct is dropped. Keep it alive for the
/// duration of the test — drop it only after agent-control has stopped.
pub struct TempBasePaths {
    base_paths: BasePaths,
    local_dir: TempDir,
    remote_dir: TempDir,
    log_dir: TempDir,
}

impl TempBasePaths {
    pub fn new() -> Self {
        let local_dir = tempdir().expect("failed to create local temp dir");
        let remote_dir = tempdir().expect("failed to create remote temp dir");
        let log_dir = tempdir().expect("failed to create log temp dir");
        let base_paths = BasePaths {
            local_dir: local_dir.path().to_path_buf(),
            remote_dir: remote_dir.path().to_path_buf(),
            log_dir: log_dir.path().to_path_buf(),
        };
        Self {
            base_paths,
            local_dir,
            remote_dir,
            log_dir,
        }
    }

    pub fn base_paths(&self) -> BasePaths {
        self.base_paths.clone()
    }

    pub fn local_dir(&self) -> PathBuf {
        self.local_dir.path().to_path_buf()
    }

    pub fn remote_dir(&self) -> PathBuf {
        self.remote_dir.path().to_path_buf()
    }

    #[allow(dead_code)]
    pub fn log_dir(&self) -> PathBuf {
        self.log_dir.path().to_path_buf()
    }
}
