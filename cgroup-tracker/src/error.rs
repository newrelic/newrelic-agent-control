use std::path::PathBuf;

/// Errors returned by the cgroup tracker.
#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    /// No usable cgroup hierarchy could be detected for the current process.
    #[error("detecting cgroup hierarchy: {0}")]
    Detect(String),
    /// A cgroupfs operation failed.
    #[error("{operation} cgroup `{path}`: {source}")]
    Io {
        /// Operation that failed (e.g. `creating`, `removing`).
        operation: &'static str,
        /// Path of the cgroup involved.
        path: PathBuf,
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
