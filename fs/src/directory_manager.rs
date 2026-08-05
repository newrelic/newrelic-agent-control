use super::utils::validate_path;
use std::fs::{DirBuilder, remove_dir_all};
use std::io;
use std::path::{Path, PathBuf};
use tracing::instrument;

/// Creates and removes directories on disk.
pub trait DirectoryManager: Send + Sync {
    /// create will create a folder
    fn create(&self, path: &Path) -> io::Result<()>;

    /// Delete the folder and its contents. If the folder does not exist it
    /// will not return an error.
    fn delete(&self, path: &Path) -> io::Result<()>;

    /// List the immediate children of `path`. Returns absolute paths. If `path` does not
    /// exist, returns an empty list (not an error).
    fn list(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}

/// [`DirectoryManager`] implementation backed by the real filesystem.
///
/// This is expected to be thread-safe since it is used in the package manager.
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

    #[instrument(skip_all, fields(path = %path.display()))]
    fn list(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        validate_path(path)?;

        let read = match std::fs::read_dir(path) {
            Ok(r) => r,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        read.map(|entry| entry.map(|e| e.path())).collect()
    }
}

impl DirectoryManagerFs {
    #[cfg(target_family = "unix")]
    fn get_directory_permissions() -> std::fs::Permissions {
        use std::os::unix::fs::PermissionsExt;

        std::fs::Permissions::from_mode(0o700)
    }
}

/// Repairs the managed Administrators-only permissions across `path` and everything beneath it,
/// re-stamping only the entries that are actually broken.
///
/// On Windows, for each entry it checks (via `win_permissions::permissions_need_repair`)
/// whether the DACL already grants the managed Administrators access. It re-stamps only entries that
/// do not — empty (denies everyone incl. SYSTEM), NULL, unreadable, or populated-but-insufficient
/// (e.g. the old `Administrators:(R,W)` with no DELETE that blocks decommission, or a non-inheritable
/// directory ACE) — the states an older agent-control left behind on upgrade (NR-601065). A
/// conforming entry is left untouched, so a healthy tree is not rewritten on every startup. It always
/// recurses into directories to find broken children, and a broken directory is stamped *before* its
/// contents are listed so it becomes listable first. Agent Control owns these files, so the rewrite
/// succeeds even on an empty DACL.
///
/// On non-Windows platforms permissions are applied at creation time, so this is a no-op. A missing
/// path is not an error.
///
/// Caveats (not currently hit in practice, but worth knowing before extending this):
/// - This walks the *whole* tree on every call, i.e. on every Agent Control startup, not just once
///   after an upgrade. `permissions_need_repair` keeps each individual check cheap (no re-stamping of
///   conforming entries), but a fleet with very large `filesystem/`/`fleet-data` trees still pays one
///   ACL read per entry on every restart, indefinitely.
/// - `path.is_dir()` and `read_dir` follow reparse points/symlinks, so a symlink planted inside a
///   managed tree would be traversed and re-ACL'd rather than skipped. Low risk given the tree is
///   Administrators-only to begin with, but there is no explicit guard against it.
#[cfg(target_family = "windows")]
pub fn ensure_permissions_recursive(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if crate::win_permissions::permissions_need_repair(path) {
        tracing::debug!(path = %path.display(), "repairing managed permissions");
        crate::win_permissions::set_file_permissions_for_administrator(path).map_err(|err| {
            io::Error::other(format!(
                "setting windows permissions for {}: {err}",
                path.display()
            ))
        })?;
    } else {
        tracing::trace!(path = %path.display(), "managed permissions intact, skipping");
    }

    if path.is_dir() {
        std::fs::read_dir(path)?
            .try_for_each(|entry| ensure_permissions_recursive(&entry?.path()))?;
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////////
// Mock
////////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "mocks")]
#[allow(missing_docs)] // test-support code
pub mod mock {
    use super::*;
    use mockall::{mock, predicate};
    use std::path::PathBuf;

    mock! {
        pub DirectoryManager {}

        impl DirectoryManager for DirectoryManager {
            fn create(&self, path: &Path) -> io::Result<()>;
            fn delete(&self, path: &Path) -> io::Result<()>;
            fn list(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
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

        pub fn should_list(&mut self, path: &Path, children: Vec<PathBuf>) {
            let path_clone = PathBuf::from(path.to_str().unwrap().to_string().as_str());
            self.expect_list()
                .with(predicate::eq(path_clone))
                .once()
                .return_once(|_| Ok(children));
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[allow(missing_docs)] // test-support code
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
    fn test_list_missing_path_returns_empty() {
        let tempdir = tempfile::tempdir().unwrap();
        let missing = tempdir.path().join("does_not_exist");
        let directory_manager = DirectoryManagerFs;

        let result = directory_manager.list(&missing).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_list_returns_immediate_children() {
        let tempdir = tempfile::tempdir().unwrap();
        let directory_manager = DirectoryManagerFs;

        let child_dir = tempdir.path().join("child_dir");
        directory_manager.create(&child_dir).unwrap();
        let child_file = tempdir.path().join("child_file");
        std::fs::write(&child_file, b"contents").unwrap();
        std::fs::write(child_dir.join("grandchild"), b"contents").unwrap();

        let mut result = directory_manager.list(tempdir.path()).unwrap();
        result.sort();

        let mut expected = vec![child_dir, child_file];
        expected.sort();
        assert_eq!(expected, result);
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
