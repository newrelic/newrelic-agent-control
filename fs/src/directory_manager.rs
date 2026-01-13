use super::utils::validate_path;
use std::fs::{DirBuilder, remove_dir_all};
use std::io;
use std::path::Path;
use tracing::instrument;

pub trait DirectoryManager {
    /// create will create a folder
    fn create(&self, path: &Path) -> io::Result<()>;

    /// Delete the folder and its contents. If the folder does not exist it
    /// will not return an error.
    fn delete(&self, path: &Path) -> io::Result<()>;
}

#[derive(Clone)]
pub struct DirectoryManagerFs;

impl DirectoryManager for DirectoryManagerFs {
    fn create(&self, path: &Path) -> io::Result<()> {
        validate_path(path)?;
        let mut directory_builder = DirBuilder::new();
        directory_builder.recursive(true);

        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::DirBuilderExt;
            use std::os::unix::fs::PermissionsExt;

            directory_builder.mode(DirectoryManagerFs::get_directory_permissions().mode());
        }

        directory_builder.create(path)?;

        #[cfg(target_family = "windows")]
        crate::win_permissions::set_file_permissions_for_administrator(path).map_err(|err| {
            io::Error::other(format!(
                "Failed to set windows permissions for {}: {}",
                path.display(),
                err
            ))
        })?;

        Ok(())
    }

    #[instrument(skip_all, fields(path = %path.display()))]
    fn delete(&self, path: &Path) -> io::Result<()> {
        validate_path(path)?;

        if !path.exists() {
            return Ok(());
        }
        remove_dir_all(path)
    }
}

impl DirectoryManagerFs {
    #[cfg(target_family = "unix")]
    fn get_directory_permissions() -> std::fs::Permissions {
        use std::os::unix::fs::PermissionsExt;

        std::fs::Permissions::from_mode(0o700)
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
            fn create(&self, path: &Path) -> io::Result<()>;
            fn delete(&self, path: &Path) -> io::Result<()>;
        }
        impl Clone for DirectoryManager {
            fn clone(&self) -> Self;
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

        pub fn should_not_create(&mut self, path: &Path, err: io::Error) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_create()
                .with(predicate::eq(path_clone))
                .return_once(|_| Err(err));
        }

        pub fn should_delete(&mut self, path: &Path) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_delete()
                .with(predicate::eq(path_clone))
                .once()
                .returning(|_| Ok(()));
        }

        pub fn should_not_delete(&mut self, path: &Path, err: io::Error) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_delete()
                .with(predicate::eq(path_clone))
                .return_once(|_| Err(err));
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
            "dots disallowed in path some/path/../with/../dots".to_string(),
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
            "dots disallowed in path some/path/../with/../dots".to_string(),
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

        #[cfg(target_family = "windows")]
        crate::win_permissions::tests::assert_windows_permissions(&path);

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
    #[ignore = "requires windows administrator"]
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
