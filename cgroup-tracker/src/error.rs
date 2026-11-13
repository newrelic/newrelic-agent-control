use std::path::{Path, PathBuf};

/// Errors returned by the cgroup tracker.
#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    /// No usable cgroup hierarchy could be detected for the current process.
    #[error("detecting cgroup hierarchy: {0}")]
    Detect(String),
    /// A cgroupfs or procfs operation failed.
    #[error("{operation} `{path}`: {source}")]
    IO {
        /// Operation that failed (e.g. `creating`, `removing`).
        operation: &'static str,
        /// Path of the cgroup involved.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Reading the command line of a process failed.
    #[error("reading command line of process {pid}: {source}")]
    Cmdline {
        /// Pid of the process involved.
        pid: i32,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The processes of a cgroup did not terminate in time.
    #[error("processes in cgroup `{path}` still alive after {attempts} attempts")]
    KillTimeout {
        /// Path of the cgroup involved.
        path: PathBuf,
        /// Number of kill attempts performed.
        attempts: u32,
    },
}

impl CgroupError {
    /// Syntactic sugar to built [Self::IO] errors.
    pub fn io(operation: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::IO {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}
