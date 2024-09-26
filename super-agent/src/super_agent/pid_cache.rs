use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::FileReader;
use fs::writer_file::{FileWriter, WriteError};
use fs::LocalFile;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use thiserror::Error;

const PROC_PATH: &str = "/proc";
const PID_FILE_PATH: &str = "/var/run/newrelic-super-agent/newrelic-super-agent.pid";
const PID_FOLDER_PERMISSIONS: u32 = 0o755;
const PID_FILE_PERMISSIONS: u32 = 0o644;

#[derive(Error, Debug)]
pub enum PIDCacheError {
    #[error("invalid PID file path")]
    InvalidFilePath,

    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: `{0}`")]
    SaveError(#[from] WriteError),

    #[error("pid-file already exists. Can't guarantee that no other agent-control is running.")]
    RunningProcessAlreadyCached,
}

/// PIDCache stores the current running pid on the file_path set, by default:
/// "/var/run/newrelic-super-agent/newrelic-super-agent.pid"
/// We use this PIDCache to ensure only one instance of the super-agent is running.
pub struct PIDCache<F = LocalFile, D = DirectoryManagerFs>
where
    F: FileWriter + FileReader,
    D: DirectoryManager,
{
    file_rw: F,
    directory_manager: D,
    file_path: PathBuf,
    proc_path: PathBuf,
}

impl<F, D> PIDCache<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(file_rw: F, directory_manager: D, file_path: PathBuf, proc_path: PathBuf) -> Self {
        PIDCache {
            file_rw,
            directory_manager,
            file_path,
            proc_path,
        }
    }
}

impl Default for PIDCache<LocalFile, DirectoryManagerFs> {
    fn default() -> Self {
        PIDCache {
            file_rw: LocalFile,
            directory_manager: DirectoryManagerFs::default(),
            file_path: PID_FILE_PATH.into(),
            proc_path: PROC_PATH.into(),
        }
    }
}

impl<F, D> PIDCache<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    /// The method store will first read the file path and if it finds content, will try to match
    /// the content (a pid) with an existing pid in the "/proc" folder from the filesystem.
    /// If one is found an error will be returned, meaning there is already an instance
    /// of the super-agent running.
    /// If no pid is found in the "/proc" folder the pid will be stored in the file cache.
    pub fn store(&self, pid: u32) -> Result<(), PIDCacheError> {
        let pid_folder = self
            .file_path
            .parent()
            .ok_or(PIDCacheError::InvalidFilePath)?;

        if !pid_folder.exists() {
            self.directory_manager
                .create(pid_folder, Permissions::from_mode(PID_FOLDER_PERMISSIONS))?;
        }

        let pid_string = self
            .file_rw
            .read(self.file_path.as_path())
            .unwrap_or_else(|_| "".to_string())
            .trim()
            .to_string();

        if !pid_string.is_empty() {
            let mut proc_path = self.proc_path.clone();
            proc_path.push(pid_string);
            if proc_path.exists() {
                return Err(PIDCacheError::RunningProcessAlreadyCached);
            }
        }

        let pid_string = format!("{}", pid);
        Ok(self.file_rw.write(
            self.file_path.as_path(),
            pid_string,
            Permissions::from_mode(PID_FILE_PERMISSIONS),
        )?)
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use fs::directory_manager::mock::MockDirectoryManagerMock;
    use fs::mock::MockLocalFile;
    use std::path::PathBuf;

    #[test]
    fn test_new_pid_when_already_running_pid_fails_storing() {
        let already_running_pid: u32 = 123;

        let temp_dir = tempfile::tempdir().unwrap();
        let mut host_proc_path = PathBuf::from(&temp_dir.path());
        host_proc_path.push(already_running_pid.to_string());

        // Create fake HOST_PROC file
        let writer = LocalFile;
        _ = writer.write(
            host_proc_path.as_path(),
            "".to_string(),
            Permissions::from_mode(PID_FILE_PERMISSIONS),
        );

        let pid_path = PathBuf::from("/an/invented/path/not-existing");
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();

        file_rw.should_read(pid_path.clone().as_path(), already_running_pid.to_string());
        dir_manager.should_create(
            pid_path.parent().unwrap(),
            Permissions::from_mode(PID_FOLDER_PERMISSIONS),
        );

        let pid_cache = PIDCache::new(
            file_rw,
            dir_manager,
            pid_path,
            host_proc_path.parent().unwrap().to_path_buf(),
        );

        let result = pid_cache.store(444);
        assert!(result.is_err());
        assert_eq!(
            String::from(
                "pid-file already exists. Can't guarantee that no other agent-control is running."
            ),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_new_pid_when_no_running_pid_stores_ok() {
        let pid_not_running_anymore: u32 = 123;
        let new_pid: u32 = 444;

        let temp_dir = tempfile::tempdir().unwrap();
        let host_proc_path = PathBuf::from(&temp_dir.path());

        let pid_path = PathBuf::from("/an/invented/path/not-existing");
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();

        file_rw.should_read(
            pid_path.clone().as_path(),
            pid_not_running_anymore.to_string(),
        );
        file_rw.should_write(
            pid_path.clone().as_path(),
            new_pid.to_string(),
            Permissions::from_mode(PID_FILE_PERMISSIONS),
        );
        dir_manager.should_create(
            pid_path.parent().unwrap(),
            Permissions::from_mode(PID_FOLDER_PERMISSIONS),
        );

        let pid_cache = PIDCache::new(
            file_rw,
            dir_manager,
            pid_path,
            host_proc_path.parent().unwrap().to_path_buf(),
        );

        let result = pid_cache.store(444);
        assert!(result.is_ok());
    }
}
