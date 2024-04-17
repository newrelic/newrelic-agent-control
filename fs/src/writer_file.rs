use std::fs::Permissions;
use std::io::Write;
use std::path::Path;
use std::{fs, io};

#[cfg(target_family = "unix")]
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;

use super::directory_manager::DirectoryManagementError;
use super::utils::{validate_path, FsError};
use thiserror::Error;

use super::LocalFile;

#[derive(Error, Debug)]
pub enum WriteError {
    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("error creating file: `{0}`")]
    ErrorCreatingFile(#[from] io::Error),

    #[error("invalid path: `{0}`")]
    InvalidPath(#[from] FsError),
}

pub trait FileWriter {
    fn write(&self, path: &Path, buf: String, permissions: Permissions) -> Result<(), WriteError>;
}

impl FileWriter for LocalFile {
    fn write(&self, path: &Path, buf: String, permissions: Permissions) -> Result<(), WriteError> {
        self.write(path, buf, permissions)
    }
}

impl LocalFile {
    #[cfg(target_family = "unix")]
    pub fn write(
        &self,
        path: &Path,
        content: String,
        permissions: Permissions,
    ) -> Result<(), WriteError> {
        validate_path(path)?;

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .mode(permissions.mode())
            .open(path)?;

        file.write_all(content.as_bytes())?;

        Ok(())
    }

    // TODO : Code below is not tested yet as Windows is not supported at this time
    #[cfg(target_family = "windows")]
    fn write(&self, path: &Path, content: String) -> Result<(), WriteError> {
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;

        Ok(())
    }
}

#[cfg(feature = "mocks")]
pub mod mock {
    ////////////////////////////////////////////////////////////////////////////////////
    // Mock
    ////////////////////////////////////////////////////////////////////////////////////
    use super::*;
    use crate::mock::MockLocalFile;
    use mockall::predicate;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    impl MockLocalFile {
        pub fn should_write(&mut self, path: &Path, content: String, permissions: Permissions) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_write()
                .with(
                    predicate::eq(path_clone),
                    predicate::eq(content),
                    predicate::eq(permissions),
                )
                .once()
                .returning(|_, _, _| Ok(()));
        }

        pub fn should_not_write(&mut self, path: &Path, content: String, permissions: Permissions) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_write()
                .with(
                    predicate::eq(path_clone),
                    predicate::eq(content),
                    predicate::eq(permissions),
                )
                .once()
                .returning(|_, _, _| {
                    Err(WriteError::ErrorCreatingFile(io::Error::from(
                        ErrorKind::PermissionDenied,
                    )))
                });
        }

        pub fn should_write_any(&mut self, times: usize) {
            self.expect_write().times(times).returning(|_, _, _| Ok(()));
        }

        pub fn should_not_write_any(&mut self, times: usize, io_err_kind: ErrorKind) {
            self.expect_write().times(times).returning(move |_, _, _| {
                Err(WriteError::ErrorCreatingFile(Error::from(io_err_kind)))
            });
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod test {
    use std::fs;
    use std::fs::Permissions;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    #[cfg(target_family = "unix")]
    #[test]
    fn test_file_writer_content_and_permissions() {
        // Prepare temp path and content for the file
        let file_name = "some_file";
        let content = "some content";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(file_name);

        // Create writer and write to path with some permissions
        let some_permissions = Permissions::from_mode(0o645);
        let writer = LocalFile;
        let write_result = writer.write(
            path.as_path(),
            content.to_string(),
            some_permissions.clone(),
        );
        assert!(write_result.is_ok());

        //assert on content
        assert_eq!(fs::read_to_string(path.clone()).unwrap(), "some content");

        // read created file permissions and assert od expected ones
        let meta = fs::metadata(path).unwrap();
        // user_has_write_access
        assert_eq!(
            some_permissions.mode() & 0o200,
            meta.permissions().mode() & 0o200
        );
        // user_has_read_write_access
        assert_eq!(
            some_permissions.mode() & 0o600,
            meta.permissions().mode() & 0o600
        );
        //group_has_read_access
        assert_eq!(
            some_permissions.mode() & 0o040,
            meta.permissions().mode() & 0o040
        );
        // others_have_exec_access
        assert_eq!(
            some_permissions.mode() & 0o001,
            meta.permissions().mode() & 0o001
        );
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_file_writer_should_not_return_error_if_file_already_exists() {
        // Prepare temp path and content for the file
        let file_name = "some_file";
        let content = "some content";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(file_name);

        // Create writer and write to path
        let writer = LocalFile;
        let write_result = writer.write(
            path.as_path(),
            content.to_string(),
            Permissions::from_mode(0o645),
        );
        assert!(write_result.is_ok());
        let write_result = writer.write(
            path.as_path(),
            content.to_string(),
            Permissions::from_mode(0o645),
        );
        assert!(write_result.is_ok());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_path_to_write_cannot_contain_dots() {
        // Prepare temp path and folder name
        let file_name = "some/path/../../etc/passwd";
        let path = PathBuf::from(file_name);
        let writer = LocalFile;

        let result = writer.write(&path, "".to_string(), Permissions::from_mode(0o645));

        assert!(result.is_err());
        assert_eq!(
            "invalid path: `dots disallowed in path `some/path/../../etc/passwd``".to_string(),
            result.unwrap_err().to_string()
        );
    }
}
