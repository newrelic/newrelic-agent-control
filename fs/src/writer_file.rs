use super::LocalFile;
use super::directory_manager::DirectoryManagementError;
use super::utils::{FsError, validate_path};
use std::io::Write;
use std::path::Path;
use std::{fs, io};
use thiserror::Error;
use tracing::instrument;

#[derive(Error, Debug)]
pub enum WriteError {
    #[error("directory error: {0}")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("error creating file: {0}")]
    ErrorCreatingFile(#[from] io::Error),

    #[error("invalid path: {0}")]
    InvalidPath(#[from] FsError),
}

pub trait FileWriter {
    fn write(&self, path: &Path, buf: String) -> Result<(), WriteError>;
}

impl FileWriter for LocalFile {
    #[instrument(skip_all, fields(path = %path.display()))]
    fn write(&self, path: &Path, content: String) -> Result<(), WriteError> {
        validate_path(path)?;

        let mut file_options = fs::OpenOptions::new();
        file_options.create(true).write(true).truncate(true);

        #[cfg(target_family = "unix")]
        {
            file_options.mode(LocalFile::get_file_permissions().mode());
        }

        file_options.open(path)?.write_all(content.as_bytes())?;
        Ok(())
    }
}

impl LocalFile {
    #[cfg(target_family = "unix")]
    fn get_file_permissions() -> Permissions {
        use std::fs::Permissions;

        Permissions::from_mode(0o600)
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
        pub fn should_write(&mut self, path: &Path, content: String) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_write()
                .with(predicate::eq(path_clone), predicate::eq(content))
                .once()
                .returning(|_, _| Ok(()));
        }

        pub fn should_not_write(&mut self, path: &Path, content: String) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_write()
                .with(predicate::eq(path_clone), predicate::eq(content))
                .once()
                .returning(|_, _| {
                    Err(WriteError::ErrorCreatingFile(io::Error::from(
                        ErrorKind::PermissionDenied,
                    )))
                });
        }

        pub fn should_write_any(&mut self, times: usize) {
            self.expect_write().times(times).returning(|_, _| Ok(()));
        }

        pub fn should_not_write_any(&mut self, times: usize, io_err_kind: ErrorKind) {
            self.expect_write().times(times).returning(move |_, _| {
                Err(WriteError::ErrorCreatingFile(Error::from(io_err_kind)))
            });
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[cfg(target_family = "unix")]
    #[test]
    fn test_file_writer_content_and_permissions() {
        use std::fs::metadata;
        use std::os::unix::fs::PermissionsExt;

        // Prepare temp path and content for the file
        let file_name = "some_file";
        let content = "some content";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(file_name);

        // Create writer and write to path with some permissions
        let writer = LocalFile;
        let write_result = writer.write(path.as_path(), content.to_string());
        assert!(write_result.is_ok());

        //assert on content
        assert_eq!(fs::read_to_string(path.clone()).unwrap(), "some content");

        // read created file permissions and assert od expected ones
        assert_eq!(
            LocalFile::get_file_permissions().mode() & 0o777,
            metadata(path).unwrap().permissions().mode() & 0o777
        );
    }

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
        let write_result = writer.write(path.as_path(), content.to_string());
        assert!(write_result.is_ok());
        let write_result = writer.write(path.as_path(), content.to_string());
        assert!(write_result.is_ok());
    }

    #[test]
    fn test_file_writer_truncate_exiting_file() {
        // Prepare temp path and content for the file
        let file_name = "some_file";
        let new_content = "new content";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(file_name);

        fs::write(path.as_path(), "older content with greater len than new").unwrap();

        // Create writer and write to path
        let writer = LocalFile;
        writer
            .write(path.as_path(), new_content.to_string())
            .expect("write failed");

        assert_eq!(fs::read_to_string(path.clone()).unwrap(), new_content);
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_path_to_write_cannot_contain_dots() {
        // Prepare temp path and folder name
        let file_name = "some/path/../../etc/passwd";
        let path = PathBuf::from(file_name);
        let writer = LocalFile;

        let result = writer.write(&path, "".to_string());

        assert!(result.is_err());
        assert_eq!(
            "invalid path: dots disallowed in path some/path/../../etc/passwd".to_string(),
            result.unwrap_err().to_string()
        );
    }
}
