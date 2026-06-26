//! Cross-platform binary self-replacement implementation.
//!
//! ## Replacement Strategy
//!
//! 1. Read the current binary's permissions
//! 2. Create a backup of the current binary
//! 3. Copy/move the new binary into a temporary file in the **same directory** as the current
//!    binary (ensures the final rename is on the same filesystem and therefore atomic)
//! 4. Atomically rename the temp file over the current binary path
//! 5. On failure, restore from backup
//!
//! The only platform difference is in step 2: on Unix the backup is a copy (the OS keeps the
//! inode alive for the running process), while on Windows it is a rename (running `.exe` files
//! cannot be deleted but can be renamed).
//!
//! ## Backup Files
//! Backup files are created as `{original_name}.{BACKUP_SUFFIX}` and are NOT automatically
//! deleted. The caller is responsible for cleanup.

use crate::{BACKUP_SUFFIX, SelfReplacer};
use std::path::{Path, PathBuf};
use std::{fs, io};
use thiserror::Error;
use tracing::{debug, error};

const TEMP_PREFIX: &str = "__temp__";

/// Errors that can occur during self-replacement operations.
#[derive(Debug, Error)]
pub enum ReplaceError {
    /// The path of the currently running executable could not be determined.
    #[error("failed to determine current executable path: {0}")]
    CurrentExeNotFound(#[source] io::Error),

    /// No file exists at the path provided for the replacement binary.
    #[error("new binary not found at {0}")]
    NewBinaryNotFound(String),

    /// Backing up the current binary before replacing it failed.
    #[error("failed to create backup: {0}")]
    BackupCreationFailed(#[source] io::Error),

    /// Renaming the new binary over the current binary path failed.
    #[error("failed to replace binary: {0}")]
    ReplaceFailed(#[source] io::Error),

    /// Restoring the original binary from its backup after a failed replacement failed.
    #[error("failed to restore backup: {0}")]
    BackupRestoreFailed(#[source] io::Error),

    /// Reading the current binary's file metadata (e.g. its permissions) failed.
    #[error("failed to read file metadata: {0}")]
    Metadata(#[source] io::Error),

    /// Copying the new binary into a temporary file next to the current binary failed.
    #[error("failed to copy new binary to temporary location: {0}")]
    TempCopyFailed(#[source] io::Error),

    /// The current executable path has no parent directory to place the temporary file in.
    #[error("current executable has no parent directory")]
    NoParentDirectory,
}

/// Platform-agnostic self-replacer implementation.
#[derive(Debug)]
pub struct BinarySelfReplacer;

impl SelfReplacer for BinarySelfReplacer {
    type Error = ReplaceError;

    fn self_replace(new_bin: impl AsRef<Path>) -> Result<(), Self::Error> {
        let new_bin = new_bin.as_ref();

        debug!(
            new_bin = %new_bin.display(),
            "Starting binary self-replacement",
        );

        if !new_bin.exists() {
            return Err(ReplaceError::NewBinaryNotFound(
                new_bin.display().to_string(),
            ));
        }

        // Get current executable path
        // Canonicalize resolves symlinks on both platforms
        let current_exe = std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_err(ReplaceError::CurrentExeNotFound)?;

        debug!(
            current_exe = %current_exe.display(),
            "Current executable path",
        );

        replace_binary(&current_exe, new_bin)
    }
}

fn replace_binary(current_exe: &Path, new_bin: &Path) -> Result<(), ReplaceError> {
    let backup = backup_path(current_exe);

    let original_permissions = {
        let metadata = current_exe.metadata().map_err(ReplaceError::Metadata)?;
        metadata.permissions()
    };

    // Create backup — strategy differs by platform:
    // Unix: copy (the OS keeps the inode alive for the running process)
    // Windows: rename/vacate (running .exe files cannot be deleted but CAN be renamed)
    debug!(
        current_exe = %current_exe.display(),
        backup = %backup.display(),
        "Creating backup"
    );

    #[cfg(unix)]
    fs::copy(current_exe, &backup).map_err(ReplaceError::BackupCreationFailed)?;

    // When the Windows image loader maps an executable into memory it opens the
    // file with [`FILE_SHARE_DELETE`] but **not** `FILE_SHARE_WRITE`. The
    // `FILE_SHARE_DELETE` flag permits other callers to request delete access on
    // the same handle — and a rename (move) internally requires delete access —
    // so a rename of a running `.exe` succeeds. A direct write or overwrite of
    // the same file would fail with `ERROR_SHARING_VIOLATION` because write
    // access is not shared.
    //
    // Reference: [`CreateFileW` — `dwShareMode` / `FILE_SHARE_DELETE`][msdn-create-file]
    //
    // [msdn-create-file]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
    //
    #[cfg(windows)]
    fs::rename(current_exe, &backup).map_err(ReplaceError::BackupCreationFailed)?;

    let new_bin_temp = create_temp_file(current_exe, new_bin, &original_permissions)?;

    debug!(
        temp_path = %new_bin_temp.display(),
        current_exe = %current_exe.display(),
        "Performing final rename"
    );

    match fs::rename(&new_bin_temp, current_exe) {
        Ok(()) => {
            debug!("Self-replacement completed successfully");
            Ok(())
        }
        Err(err) => {
            error!(
                error = %err,
                "Rename failed, attempting rollback"
            );

            // Attempt to restore backup
            if let Err(restore_err) = fs::rename(&backup, current_exe) {
                error!(
                    backup = %backup.display(),
                    error = %restore_err,
                    "Failed to restore backup"
                );
                return Err(ReplaceError::BackupRestoreFailed(restore_err));
            }
            debug!("Backup restored successfully");

            Err(if err.kind() == io::ErrorKind::NotFound {
                ReplaceError::NewBinaryNotFound(new_bin_temp.display().to_string())
            } else {
                ReplaceError::ReplaceFailed(err)
            })
        }
    }
}

/// Creates a backup path by appending .{BACKUP_SUFFIX} to the full filename.
fn backup_path(exe_path: &Path) -> PathBuf {
    let filename = exe_path
        .file_name()
        .map(|f| format!("{}.{}", f.to_string_lossy(), BACKUP_SUFFIX))
        .unwrap_or_else(|| format!("replaced_binary.{}", BACKUP_SUFFIX));

    exe_path.with_file_name(filename)
}

/// Stages the new binary into a temporary file in the same directory as `current_exe`.
/// Using the same directory ensures the final rename is on the same filesystem and atomic.
fn create_temp_file(
    current_exe: &Path,
    new_bin: &Path,
    permissions: &fs::Permissions,
) -> Result<tempfile::TempPath, ReplaceError> {
    let parent_dir = current_exe
        .parent()
        .ok_or(ReplaceError::NoParentDirectory)?;

    let prefix = if let Some(stem) = current_exe.file_stem() {
        format!(".{}.{}", stem.to_string_lossy(), TEMP_PREFIX)
    } else {
        format!(".{}", TEMP_PREFIX)
    };

    // Generate a unique temp path in the same directory as current_exe
    let temp_path_builder = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent_dir)
        .map_err(ReplaceError::TempCopyFailed)?;

    // Get the path and immediately convert to TempPath (for auto-cleanup)
    let temp_path = temp_path_builder.into_temp_path();

    debug!(
        new_bin = %new_bin.display(),
        temp_path = %temp_path.display(),
        "Copying new binary to temp location"
    );

    // Try to rename (move) new_bin to temp location first
    // If on same filesystem, this is atomic and fast (no copy)
    // If cross-filesystem, fall back to copy
    match fs::rename(new_bin, &temp_path) {
        Ok(()) => {
            debug!("New binary moved successfully");
        }
        Err(_) => {
            debug!("Rename failed, falling back to copy");
            fs::copy(new_bin, &temp_path).map_err(ReplaceError::TempCopyFailed)?;
        }
    }

    // Set correct permissions (from original binary)
    fs::set_permissions(&temp_path, permissions.clone()).map_err(ReplaceError::TempCopyFailed)?;

    Ok(temp_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_backup_path() {
        // Test basic path without extension
        let exe_path = PathBuf::from("/some/path/to/binary");
        let backup = backup_path(&exe_path);

        assert_eq!(
            backup,
            PathBuf::from("/some/path/to/binary.bak"),
            "Backup path should append .bak to full filename"
        );

        // Test with extension (e.g., .exe on Windows)
        let exe_with_ext = PathBuf::from("/some/path/to/binary.exe");
        let backup_with_ext = backup_path(&exe_with_ext);

        assert_eq!(
            backup_with_ext,
            PathBuf::from("/some/path/to/binary.exe.bak"),
            "Backup should append .bak, not replace extension"
        );
    }

    #[test]
    fn test_replace_binary_overrides_stale_backup() {
        let temp_dir = TempDir::new().unwrap();
        let current_exe = temp_dir.path().join("program");
        let new_bin = temp_dir.path().join("program-new");
        let backup = backup_path(&current_exe);

        fs::write(&current_exe, b"old binary").unwrap();
        fs::write(&new_bin, b"new binary").unwrap();
        fs::write(&backup, b"stale backup from previous run").unwrap();

        replace_binary(&current_exe, &new_bin).unwrap();

        assert_eq!(fs::read(&backup).unwrap(), b"old binary");
        assert_eq!(fs::read(&current_exe).unwrap(), b"new binary");
    }

    #[test]
    fn test_replace_fails_if_current_exe_missing() {
        let temp_dir = TempDir::new().unwrap();
        let current_exe = temp_dir.path().join("program");
        let new_bin = temp_dir.path().join("program-new");

        fs::write(&new_bin, b"new binary").unwrap();

        replace_binary(&current_exe, &new_bin)
            .expect_err("Error expected when the current binary is missing");

        assert!(!backup_path(&current_exe).exists());
    }

    #[test]
    fn test_replace_fails_if_new_bin_missing() {
        let temp_dir = TempDir::new().unwrap();
        let new_bin = temp_dir.path().join("program-new");

        // Don't create new_bin - it should not exist
        let result = BinarySelfReplacer::self_replace(&new_bin);
        // Should fail with NewBinaryNotFound at the early validation check
        assert_matches!(result, Err(ReplaceError::NewBinaryNotFound(_)));
    }

    #[test]
    fn test_create_temp_file_no_collision() {
        let temp_dir = TempDir::new().unwrap();
        let current_exe = temp_dir.path().join("current_binary");
        let new_bin = temp_dir.path().join("new_binary");

        fs::write(&current_exe, b"old").unwrap();

        // Get default permissions for test
        let permissions = fs::metadata(&current_exe).unwrap().permissions();

        // Create multiple temp files to verify they don't collide
        // Recreate new_bin before each call since create_temp_file may move it
        fs::write(&new_bin, b"new").unwrap();
        let temp_path1 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();

        fs::write(&new_bin, b"new").unwrap();
        let temp_path2 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();

        fs::write(&new_bin, b"new").unwrap();
        let temp_path3 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();

        // All should exist simultaneously
        assert!(temp_path1.exists());
        assert!(temp_path2.exists());
        assert!(temp_path3.exists());

        // All should have different names (collision prevention)
        assert_ne!(temp_path1.as_ref() as &Path, temp_path2.as_ref() as &Path);
        assert_ne!(temp_path2.as_ref() as &Path, temp_path3.as_ref() as &Path);
        assert_ne!(temp_path1.as_ref() as &Path, temp_path3.as_ref() as &Path);

        // All should have the correct content
        assert_eq!(fs::read(&temp_path1).unwrap(), b"new");
        assert_eq!(fs::read(&temp_path2).unwrap(), b"new");
        assert_eq!(fs::read(&temp_path3).unwrap(), b"new");

        // All should have the correct prefix
        let expected_prefix = format!(".current_binary.{}", TEMP_PREFIX);
        let file_name1 = temp_path1.file_name().unwrap().to_str().unwrap();
        let file_name2 = temp_path2.file_name().unwrap().to_str().unwrap();
        let file_name3 = temp_path3.file_name().unwrap().to_str().unwrap();
        assert!(file_name1.starts_with(&expected_prefix));
        assert!(file_name2.starts_with(&expected_prefix));
        assert!(file_name3.starts_with(&expected_prefix));
    }

    #[test]
    fn test_permission_preservation() {
        let temp_dir = TempDir::new().unwrap();
        let current_exe = temp_dir.path().join("current_binary");
        let new_bin = temp_dir.path().join("new_binary");

        fs::write(&current_exe, b"old binary").unwrap();
        fs::write(&new_bin, b"new binary").unwrap();

        // Give current_exe and new_bin opposite readonly flags so we can distinguish them.
        let mut current_perms = fs::metadata(&current_exe).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        current_perms.set_readonly(false);
        fs::set_permissions(&current_exe, current_perms).unwrap();

        let mut new_perms = fs::metadata(&new_bin).unwrap().permissions();
        new_perms.set_readonly(true);
        fs::set_permissions(&new_bin, new_perms).unwrap();

        let original_permissions = fs::metadata(&current_exe).unwrap().permissions();
        let temp_path = create_temp_file(&current_exe, &new_bin, &original_permissions).unwrap();

        // Permissions must come from current_exe (writable), not new_bin (read-only).
        assert!(
            !fs::metadata(&temp_path).unwrap().permissions().readonly(),
            "Temp file should have original binary's permissions, not new binary's"
        );
        assert_eq!(fs::read(&temp_path).unwrap(), b"new binary");
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::*;
        use std::fs::{read, remove_file, write};
        use std::os::windows::ffi::OsStrExt;
        use tempfile::tempdir;
        use windows::Win32::Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, OPEN_EXISTING,
        };

        struct TestFiles {
            _dir: TempDir,
            current_exe: PathBuf,
            new_bin: PathBuf,
        }

        impl TestFiles {
            fn new() -> Self {
                let dir = tempdir().unwrap();
                let current_exe = dir.path().join("program.exe");
                let new_bin = dir.path().join("program-new.exe");
                Self {
                    _dir: dir,
                    current_exe,
                    new_bin,
                }
            }
        }

        /// Holds a `HANDLE` to a file opened with specific sharing flags to simulate
        /// different Windows file lock scenarios in tests.
        struct SimulatedLock(windows::Win32::Foundation::HANDLE);

        impl SimulatedLock {
            /// Opens the file with `FILE_SHARE_READ | FILE_SHARE_DELETE`, mimicking the
            /// sharing flags that the Windows image loader uses for a running `.exe`.
            /// While this guard is alive, renaming the file succeeds but writing is blocked.
            fn execution_lock(path: &Path) -> io::Result<Self> {
                Self::open(path, FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
            }

            /// Opens the file with `FILE_SHARE_READ` only (no `FILE_SHARE_DELETE`),
            /// preventing any other caller from renaming or deleting the file while
            /// this guard is alive.
            fn delete_lock(path: &Path) -> io::Result<Self> {
                Self::open(path, FILE_SHARE_READ.0)
            }

            fn open(path: &Path, share_mode: u32) -> io::Result<Self> {
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let handle = unsafe {
                    CreateFileW(
                        windows::core::PCWSTR(wide.as_ptr()),
                        GENERIC_READ.0,
                        FILE_SHARE_MODE(share_mode),
                        None,
                        OPEN_EXISTING,
                        Default::default(),
                        None,
                    )
                }
                .map_err(io::Error::from)?;

                if handle == INVALID_HANDLE_VALUE {
                    return Err(io::Error::last_os_error());
                }

                Ok(Self(handle))
            }
        }

        impl Drop for SimulatedLock {
            fn drop(&mut self) {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(self.0);
                }
            }
        }

        #[test]
        fn test_replace_binary_succeeds_with_execution_lock() {
            let files = TestFiles::new();
            write(&files.current_exe, b"old binary").unwrap();
            write(&files.new_bin, b"new binary").unwrap();
            let _lock = SimulatedLock::execution_lock(&files.current_exe).unwrap();

            replace_binary(&files.current_exe, &files.new_bin).unwrap();

            assert_eq!(read(&files.current_exe).unwrap(), b"new binary");
            assert!(!files.new_bin.exists(), "new_bin should have been moved");
            assert!(
                backup_path(&files.current_exe).exists(),
                "backup should be left"
            );
        }

        #[test]
        fn test_replace_binary_succeeds_despite_delete_lock_on_new_bin() {
            // A delete-lock on new_bin prevents rename but not reads, so create_temp_file
            // falls back to copy and the replacement still succeeds.
            let files = TestFiles::new();
            write(&files.current_exe, b"old binary").unwrap();
            write(&files.new_bin, b"new binary").unwrap();

            let _lock = SimulatedLock::delete_lock(&files.new_bin).unwrap();

            replace_binary(&files.current_exe, &files.new_bin).unwrap();

            assert_eq!(read(&files.current_exe).unwrap(), b"new binary");
        }

        #[test]
        fn test_simulated_lock_helper() {
            let files = TestFiles::new();
            write(&files.new_bin, b"new binary").unwrap();
            {
                let _lock = SimulatedLock::delete_lock(&files.new_bin).unwrap();
                remove_file(&files.new_bin).expect_err("file should be locked for delete/rename");
            }

            {
                let _lock = SimulatedLock::execution_lock(&files.new_bin).unwrap();
                write(&files.new_bin, b"foo").expect_err("should be locked against writes");
            }
        }
    }
}
