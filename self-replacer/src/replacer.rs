//! Cross-platform binary self-replacement implementation.
//!
//! ## Replacement Strategy
//!
//! ### Unix/Linux
//! Uses a temporary file approach to ensure atomic replacement:
//! 1. Creates a backup of the current binary
//! 2. Creates a temporary file in the same directory as the current binary
//! 3. Copies the new binary to the temporary location
//! 4. Preserves the original file permissions
//! 5. Atomically renames the temporary file to replace the current binary
//! 6. On failure, attempts to restore from backup
//!
//! The temp file ensures the final rename is atomic even when the new binary
//! is on a different filesystem (e.g., `/usr/bin/` vs `/var/lib/`).
//!
//! ### Windows
//! Uses two rename operations:
//! 1. Move the currently running binary to a backup location
//! 2. Move the new binary into the now-vacated slot
//! 3. On failure, restore the backup
//!
//! Windows allows renaming a running executable because the image loader opens
//! files with `FILE_SHARE_DELETE` (but not `FILE_SHARE_WRITE`).
//!
//! ## Backup Files
//! Backup files are created as `{original_name}.{BACKUP_SUFFIX}` and are NOT automatically
//! deleted. The caller is responsible for cleanup.

use crate::{BACKUP_SUFFIX, SelfReplacer};
use std::path::{Path, PathBuf};
use std::{fs, io};
use thiserror::Error;
use tracing::{debug, error};

#[cfg(unix)]
const TEMP_PREFIX: &str = "__temp__";

/// Errors that can occur during self-replacement operations.
#[derive(Debug, Error)]
pub enum ReplaceError {
    #[error("failed to determine current executable path")]
    CurrentExeNotFound(#[source] io::Error),

    #[error("new binary not found at {0}")]
    NewBinaryNotFound(String),

    #[error("failed to create backup")]
    BackupCreationFailed(#[source] io::Error),

    #[error("failed to replace binary")]
    ReplaceFailed(#[source] io::Error),

    #[error("failed to restore backup: {0}")]
    BackupRestoreFailed(#[source] io::Error),

    // Unix-specific errors
    #[cfg(unix)]
    #[error("failed to read file metadata")]
    Metadata(#[source] io::Error),

    #[cfg(unix)]
    #[error("failed to copy new binary to temporary location")]
    TempCopyFailed(#[source] io::Error),

    #[cfg(unix)]
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

/// Unified replacement implementation with platform-specific sections.
fn replace_binary(current_exe: &Path, new_bin: &Path) -> Result<(), ReplaceError> {
    let backup = backup_path(current_exe);

    #[cfg(unix)]
    let original_permissions = {
        let metadata = current_exe.metadata().map_err(ReplaceError::Metadata)?;
        metadata.permissions()
    };

    // Create backup
    // Unix: Copy (keeps original in place) - running executables can be replaced/deleted,
    //       the OS keeps the inode alive for the running process
    // Windows: Move (vacates the slot) - running .exe files are locked and cannot be deleted,
    //          but CAN be renamed. Renaming frees up the original path for the new binary
    debug!(
        current_exe = %current_exe.display(),
        backup = %backup.display(),
        "Creating backup"
    );

    #[cfg(unix)]
    fs::copy(current_exe, &backup).map_err(ReplaceError::BackupCreationFailed)?;

    #[cfg(windows)]
    fs::rename(current_exe, &backup).map_err(ReplaceError::BackupCreationFailed)?;

    // Prepare source for final rename
    // Unix: Create temp file in same dir as current_exe (ensures atomic rename across filesystems)
    // Windows: Use new_bin directly
    #[cfg(unix)]
    let temp_path = create_temp_file(current_exe, new_bin, &original_permissions)?;

    let source: &Path = {
        #[cfg(unix)]
        {
            &temp_path
        }
        #[cfg(windows)]
        {
            new_bin
        }
    };

    debug!(
        source = %source.display(),
        current_exe = %current_exe.display(),
        "Performing final rename"
    );

    match fs::rename(source, current_exe) {
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
                ReplaceError::NewBinaryNotFound(source.display().to_string())
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

/// Creates a temporary file in the same directory and copies the new binary to it.
/// Unix-only: Required for atomic rename across filesystems.
#[cfg(unix)]
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

    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

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

            // Create current binary with specific permissions (0o755)
            fs::write(&current_exe, b"old binary").unwrap();
            let mut perms = fs::metadata(&current_exe).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&current_exe, perms).unwrap();

            // Create new binary with DIFFERENT permissions (0o644)
            fs::write(&new_bin, b"new binary").unwrap();
            let mut new_perms = fs::metadata(&new_bin).unwrap().permissions();
            new_perms.set_mode(0o644);
            fs::set_permissions(&new_bin, new_perms).unwrap();

            // Call create_temp_file to test permission preservation
            let original_permissions = fs::metadata(&current_exe).unwrap().permissions();
            let temp_path =
                create_temp_file(&current_exe, &new_bin, &original_permissions).unwrap();

            // Verify permissions are preserved from ORIGINAL (0o755), not from new binary (0o644)
            let temp_metadata = fs::metadata(&temp_path).unwrap();
            assert_eq!(
                temp_metadata.permissions().mode() & 0o777,
                0o755,
                "Temp file should have original binary's permissions, not new binary's"
            );

            // Verify content is from new binary
            assert_eq!(fs::read(&temp_path).unwrap(), b"new binary");
        }
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
        fn test_rollback_when_replacement_failed_due_to_lock() {
            let files = TestFiles::new();
            write(&files.current_exe, b"old binary").unwrap();
            write(&files.new_bin, b"new binary").unwrap();

            let _lock = SimulatedLock::delete_lock(&files.new_bin).unwrap();

            let err = replace_binary(&files.current_exe, &files.new_bin).unwrap_err();

            assert_matches!(err, ReplaceError::ReplaceFailed(_));
            assert_eq!(read(&files.current_exe).unwrap(), b"old binary");
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
