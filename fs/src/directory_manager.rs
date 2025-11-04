use super::utils::{FsError, validate_path};
#[cfg(target_family = "unix")]
use std::fs::Permissions;
use std::fs::{DirBuilder, remove_dir_all};
use std::path::Path;
use thiserror::Error;
use tracing::instrument;

#[derive(Error, Debug)]
pub enum DirectoryManagementError {
    #[error("cannot create directory '{0}' : {1}")]
    ErrorCreatingDirectory(String, String),

    #[error("cannot delete directory: {0}")]
    ErrorDeletingDirectory(String),

    #[error("invalid directory: {0}")]
    InvalidDirectory(#[from] FsError),
}

pub trait DirectoryManager {
    /// create will create a folder
    fn create(&self, path: &Path) -> Result<(), DirectoryManagementError>;

    /// Delete the folder and its contents. If the folder does not exist it
    /// will not return an error.
    fn delete(&self, path: &Path) -> Result<(), DirectoryManagementError>;
}

pub struct DirectoryManagerFs;

impl DirectoryManager for DirectoryManagerFs {
    fn create(&self, path: &Path) -> Result<(), DirectoryManagementError> {
        validate_path(path)?;
        let mut directory_builder = DirBuilder::new();
        directory_builder.recursive(true);
        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::DirBuilderExt;
            use std::os::unix::fs::PermissionsExt;
            directory_builder.mode(DirectoryManagerFs::get_directory_permissions().mode());
        }
        let directory_creation = directory_builder.create(path);
        match directory_creation {
            Err(e) => Err(DirectoryManagementError::ErrorCreatingDirectory(
                path.to_str().unwrap().to_string(),
                e.to_string(),
            )),
            _ => Ok(()),
        }
    }

    #[instrument(skip_all, fields(path = %path.display()))]
    fn delete(&self, path: &Path) -> Result<(), DirectoryManagementError> {
        validate_path(path)?;

        if !path.exists() {
            return Ok(());
        }
        match remove_dir_all(path) {
            Err(e) => Err(DirectoryManagementError::ErrorDeletingDirectory(
                e.to_string(),
            )),
            _ => Ok(()),
        }
    }
}

impl DirectoryManagerFs {
    #[cfg(target_family = "unix")]
    fn get_directory_permissions() -> Permissions {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};
        Permissions::from_mode(0o700)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Mock
////////////////////////////////////////////////////////////////////////////////////
impl Clone for DirectoryManagementError {
    fn clone(&self) -> Self {
        match self {
            DirectoryManagementError::ErrorCreatingDirectory(path, s) => {
                DirectoryManagementError::ErrorCreatingDirectory(path.clone(), s.to_string())
            }
            DirectoryManagementError::ErrorDeletingDirectory(s) => {
                DirectoryManagementError::ErrorDeletingDirectory(s.to_string())
            }
            DirectoryManagementError::InvalidDirectory(s) => {
                DirectoryManagementError::InvalidDirectory(s.clone())
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Mock
////////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "mocks")]
pub mod mock {
    use super::*;
    use mockall::{mock, predicate};
    use std::path::PathBuf;

    mock! {
        pub DirectoryManager {}

        impl DirectoryManager for DirectoryManager {
            fn create(&self, path: &Path) -> Result<(), DirectoryManagementError>;
            fn delete(&self, path: &Path) -> Result<(), DirectoryManagementError>;
        }
    }

    impl MockDirectoryManager {
        pub fn should_create(&mut self, path: &Path) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_create()
                .with(predicate::eq(path_clone))
                .once()
                .returning(|_| Ok(()));
        }

        pub fn should_not_create(&mut self, path: &Path, err: DirectoryManagementError) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_create()
                .with(predicate::eq(path_clone))
                .once()
                .returning(move |_| Err(err.clone()));
        }

        pub fn should_delete(&mut self, path: &Path) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_delete()
                .with(predicate::eq(path_clone))
                .once()
                .returning(|_| Ok(()));
        }

        pub fn should_not_delete(&mut self, path: &Path, err: DirectoryManagementError) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_delete()
                .with(predicate::eq(path_clone))
                .once()
                .returning(move |_| Err(err.clone()));
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod tests {
    use super::DirectoryManagerFs;
    use crate::directory_manager::DirectoryManager;
    use std::path::PathBuf;

    #[test]
    fn test_path_to_create_cannot_contain_dots() {
        // Prepare temp path and folder name
        let folder_name = "some/path/../with/../dots";
        let path = PathBuf::from(folder_name);
        let directory_manager = DirectoryManagerFs;

        let result = directory_manager.create(&path);

        assert!(result.is_err());
        assert_eq!(
            "invalid directory: dots disallowed in path some/path/../with/../dots".to_string(),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_path_to_delete_cannot_contain_dots() {
        // Prepare temp path and folder name
        let folder_name = "some/path/../with/../dots";
        let path = PathBuf::from(folder_name);
        let directory_manager = DirectoryManagerFs;

        let result = directory_manager.delete(&path);

        assert!(result.is_err());
        assert_eq!(
            "invalid directory: dots disallowed in path some/path/../with/../dots".to_string(),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_folder_creation() {
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("some_file");

        // Create directory manager and create directory with some permissions
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path());
        assert!(create_result.is_ok());

        // read created folder permissions and assert od expected ones
        #[cfg(target_family = "unix")]
        {
            use std::fs::metadata;
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                DirectoryManagerFs::get_directory_permissions().mode() & 0o777,
                metadata(&path).unwrap().permissions().mode() & 0o777
            );
        }
        assert!(path.exists());
    }

    #[test]
    fn test_folder_creation_should_not_fail_if_exists() {
        // Prepare temp path and folder name
        let folder_name = "some_file";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(folder_name);

        // Create directory manager and create directory with some permissions
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path());
        assert!(create_result.is_ok());
        let create_result = directory_manager.create(path.as_path());
        assert!(create_result.is_ok());
    }

    #[test]
    fn test_folder_deletion() {
        // Prepare temp path and folder name
        let folder_name = "some_file";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(folder_name);

        // Create directory manager and create directory with some permissions
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path());
        assert!(create_result.is_ok());
        let delete_result = directory_manager.delete(path.as_path());
        assert!(delete_result.is_ok());
        let create_result = directory_manager.create(path.as_path());
        assert!(create_result.is_ok());
    }
}
