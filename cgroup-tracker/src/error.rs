use std::path::{Path, PathBuf};

/// Errors returned by the cgroup tracker.
#[derive(Debug, thiserror::Error)]
pub enum CgroupTrackerError {
    /// No usable cgroup hierarchy could be detected for the current process.
    #[error("detecting cgroup hierarchy: {0}")]
    Detect(#[source] DetectError),
    /// A cgroupfs or procfs operation failed.
    #[error("{operation} `{path}`: {source}")]
    IO {
        /// Operation that failed.
        operation: CgroupOperation,
        /// Path of the cgroup involved.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The cgroup path cannot be passed to the kernel.
    #[error("invalid cgroup path '{path}': {source}")]
    InvalidPath {
        /// Offending path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::ffi::NulError,
    },
    /// Spawning the command inside the cgroup failed.
    #[error("spawning '{program}' in cgroup '{path}': {source}")]
    Spawn {
        /// Program that could not be spawned.
        program: String,
        /// Path of the cgroup involved.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Sending SIGKILL to a member of a cgroup failed.
    #[error("killing process {pid} of cgroup '{path}': {source}")]
    Signal {
        /// Pid of the process that could not be killed.
        pid: i32,
        /// Path of the cgroup involved.
        path: PathBuf,
        /// Errno returned by `kill(2)`.
        #[source]
        source: nix::errno::Errno,
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

impl CgroupTrackerError {
    /// Syntactic sugar to build [Self::IO] errors.
    pub fn io(operation: CgroupOperation, path: &Path) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::IO {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

/// The given string is not a valid [CgroupName](crate::CgroupName).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid cgroup name '{name}': {reason}")]
pub struct InvalidCgroupName {
    pub(crate) name: String,
    pub(crate) reason: &'static str,
}

/// Failure to resolve the cgroup hierarchy of the current process.
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    /// Neither a `cgroup2` mount nor a v1 `name=systemd` mount was found.
    #[error("no cgroup2 or v1 `name=systemd` cgroup mount found")]
    NoSupportedHierarchy,
    /// The process does not belong to any mounted cgroup hierarchy.
    #[error("process is not a member of any mounted cgroup hierarchy")]
    NotInAnyHierarchy,
    /// The cgroup of the process is not reachable from the selected mount.
    #[error("cgroup path '{cgroup_path}' is outside the mount root '{mount_root}'")]
    OutsideMountRoot {
        /// Cgroup path of the process, as read from `/proc/self/cgroup`.
        cgroup_path: String,
        /// Root of the selected mount.
        mount_root: PathBuf,
    },
}

/// Cgroupfs or procfs operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum CgroupOperation {
    /// Opening a file.
    #[strum(to_string = "opening cgroup file")]
    Open,
    /// Creating a cgroup.
    #[strum(to_string = "creating a cgroup")]
    Create,
    /// Removing a cgroup.
    #[strum(to_string = "removing a cgroup")]
    Remove,
    /// Writing the cgroup kill file.
    #[strum(to_string = "writing the cgroup kill file")]
    WriteKillFile,
    /// Reading a cgroup file or directory.
    #[strum(to_string = "reading a cgroup file")]
    Read,
}
