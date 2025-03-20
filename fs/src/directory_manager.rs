use super::utils::{validate_path, FsError};
use std::fs::{remove_dir_all, DirBuilder, Permissions};
#[cfg(target_family = "unix")]
use std::os::unix::fs::DirBuilderExt;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use thiserror::Error;
use tracing::instrument;

#[derive(Error, Debug)]
pub enum DirectoryManagementError {
    #[error("cannot create directory `{0}` : `{1}`")]
    ErrorCreatingDirectory(String, String),

    #[error("cannot delete directory: `{0}`")]
    ErrorDeletingDirectory(String),

    #[error("invalid directory: `{0}`")]
    InvalidDirectory(#[from] FsError),
}

pub trait DirectoryManager {
    #[cfg(target_family = "unix")]
    /// create will create a folder
    fn create(&self, path: &Path, permissions: Permissions)
        -> Result<(), DirectoryManagementError>;

    /// Delete the folder and its contents. If the folder does not exist it
    /// will not return an error.
    fn delete(&self, path: &Path) -> Result<(), DirectoryManagementError>;
}

pub struct DirectoryManagerFs;

impl DirectoryManager for DirectoryManagerFs {
    #[cfg(target_family = "unix")]
    fn create(
        &self,
        path: &Path,
        permissions: Permissions,
    ) -> Result<(), DirectoryManagementError> {
        validate_path(path)?;

        let directory_creation = DirBuilder::new()
            .mode(permissions.mode())
            .recursive(true)
            .create(path);
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

    fn clone_from(&mut self, _: &Self) {
        unimplemented!()
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
        pub DirectoryManagerMock {}

        #[cfg(target_family = "unix")]
        impl DirectoryManager for DirectoryManagerMock {
            fn create(&self, path: &Path, permissions: Permissions) -> Result<(), DirectoryManagementError>;
            fn delete(&self, path: &Path) -> Result<(), DirectoryManagementError>;
        }
    }

    impl MockDirectoryManagerMock {
        pub fn should_create(&mut self, path: &Path, permissions: Permissions) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_create()
                .with(predicate::eq(path_clone), predicate::eq(permissions))
                .once()
                .returning(|_, _| Ok(()));
        }

        pub fn should_not_create(
            &mut self,
            path: &Path,
            permissions: Permissions,
            err: DirectoryManagementError,
        ) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_create()
                .with(predicate::eq(path_clone), predicate::eq(permissions))
                .once()
                .returning(move |_, _| Err(err.clone()));
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
    use std::fs;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::DirectoryManagerFs;
    use crate::directory_manager::DirectoryManager;

    #[test]
    fn test_path_to_create_cannot_contain_dots() {
        // Prepare temp path and folder name
        let folder_name = "some/path/../with/../dots";
        let path = PathBuf::from(folder_name);
        let directory_manager = DirectoryManagerFs;

        let result = directory_manager.create(&path, Permissions::from_mode(0o645));

        assert!(result.is_err());
        assert_eq!(
            "invalid directory: `dots disallowed in path `some/path/../with/../dots``".to_string(),
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
            "invalid directory: `dots disallowed in path `some/path/../with/../dots``".to_string(),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_folder_creation_and_permissions() {
        // Prepare temp path and folder name
        let folder_name = "some_file";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(folder_name);

        // Create directory manager and create directory with some permissions
        let some_permissions = Permissions::from_mode(0o645);
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path(), some_permissions.clone());
        assert!(create_result.is_ok());

        // read created folder permissions and assert od expected ones
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

    #[test]
    fn test_folder_creation_should_not_fail_if_exists() {
        // Prepare temp path and folder name
        let folder_name = "some_file";
        // tempdir gets automatically removed on drop
        let tempdir = tempfile::tempdir().unwrap();
        let mut path = PathBuf::from(&tempdir.path());
        path.push(folder_name);

        // Create directory manager and create directory with some permissions
        let some_permissions = Permissions::from_mode(0o645);
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path(), some_permissions.clone());
        assert!(create_result.is_ok());
        let create_result = directory_manager.create(path.as_path(), some_permissions.clone());
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
        let some_permissions = Permissions::from_mode(0o645);
        let directory_manager = DirectoryManagerFs;
        let create_result = directory_manager.create(path.as_path(), some_permissions.clone());
        assert!(create_result.is_ok());
        let delete_result = directory_manager.delete(path.as_path());
        assert!(delete_result.is_ok());
        let create_result = directory_manager.create(path.as_path(), some_permissions.clone());
        assert!(create_result.is_ok());
    }
}
