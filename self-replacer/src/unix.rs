//! Unix/Linux implementation of binary self-replacement.
//!
//! This module provides atomic binary replacement for Unix-like systems using
//! the `rename()` system call, which is atomic within the same filesystem.
//!
//! The implementation:
//! 1. Creates a backup of the current binary
//! 2. Creates a temporary file in the same directory as the current binary
//! 3. Copies the new binary to the temporary location
//! 4. Preserves the original file permissions
//! 5. Atomically renames the temporary file to replace the current binary
//! 6. On failure, attempts to restore from backup

use crate::{BACKUP_SUFIX, SelfReplacer};
use std::path::{Path, PathBuf};
use std::{fs, io};
use thiserror::Error;
use tracing::debug;

const TEMP_PREFIX: &str = "__temp__";

/// Errors that can occur during self-replacement operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to determine current executable path")]
    CurrentExeNotFound(#[source] io::Error),

    #[error("failed to read file metadata")]
    Metadata(#[source] io::Error),

    #[error("failed to create backup")]
    BackupCreationFailed(#[source] io::Error),

    #[error("failed to copy new binary to temporary location")]
    TempCopyFailed(#[source] io::Error),

    #[error("failed to replace binary")]
    ReplaceFailed(#[source] io::Error),

    #[error("failed to restore backup")]
    BackupRestoreFailed(#[source] io::Error),

    #[error("current executable has no parent directory")]
    NoParentDirectory,

    #[error("new binary not found at {0}")]
    NewBinaryNotFound(String),
}

/// Unix implementation of [`SelfReplacer`].
#[derive(Debug)]
pub struct UnixSelfReplacer;

impl SelfReplacer for UnixSelfReplacer {
    type Error = Error;

    fn self_replace(new_bin: impl AsRef<Path>) -> Result<(), Self::Error> {
        let new_bin = new_bin.as_ref();
        debug!(
            "starting self-replacement with new binary: {}",
            new_bin.display()
        );

        if !new_bin.exists() {
            return Err(Error::NewBinaryNotFound(new_bin.display().to_string()));
        }

        let current_exe = std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_err(Error::CurrentExeNotFound)?;
        debug!("current executable path: {}", current_exe.display());

        let original_metadata = current_exe.metadata().map_err(Error::Metadata)?;
        let original_permissions = original_metadata.permissions();

        let backup_path = create_backup(&current_exe)?;
        debug!("backup created at: {}", backup_path.display());

        let temp_file = create_temp_file(&current_exe, new_bin, &original_permissions)?;
        debug!("temporary file created at: {}", temp_file.path().display());

        // Convert to TempPath for automatic cleanup on error
        let temp_path = temp_file.into_temp_path();

        // Atomically rename temporary file to replace current binary
        // On Unix, rename() is atomic within the same filesystem
        debug!(
            "performing atomic rename: {} -> {}",
            temp_path.display(),
            current_exe.display()
        );
        match fs::rename(&temp_path, &current_exe) {
            Ok(()) => {
                debug!("self-replacement completed successfully");
                Ok(())
            }
            Err(e) => {
                debug!("rename failed: {}, attempting rollback from backup", e);
                // Replacement failed, attempt to restore from backup
                // temp_path will auto-delete on drop
                if let Err(restore_err) = fs::rename(&backup_path, &current_exe) {
                    debug!("backup restoration failed: {}", restore_err);
                    return Err(Error::BackupRestoreFailed(restore_err));
                }

                debug!("backup restored successfully");
                Err(Error::ReplaceFailed(e))
            }
        }
    }
}

/// Creates a backup of the current binary.
///
/// The backup is created in the same directory with a `.backup` extension.
fn create_backup(current_exe: &Path) -> Result<PathBuf, Error> {
    let backup_path = current_exe.with_extension(BACKUP_SUFIX);
    debug!(
        "copying {} -> {}",
        current_exe.display(),
        backup_path.display()
    );

    fs::copy(current_exe, &backup_path).map_err(Error::BackupCreationFailed)?;

    Ok(backup_path)
}

/// Creates a temporary file in the same directory and copies the new binary to it.
///
/// Uses the `tempfile` crate to ensure unique naming and proper cleanup semantics.
/// The temporary file is created with a prefix based on the original binary name.
/// The temp file is created in the same directory as the current executable to ensure
/// that `rename()` will be atomic (requires same filesystem).
///
/// The permissions from the original binary are applied during file creation.
///
/// Returns a `NamedTempFile` which will automatically clean itself up on drop if not persisted.
fn create_temp_file(
    current_exe: &Path,
    new_bin: &Path,
    permissions: &fs::Permissions,
) -> Result<tempfile::NamedTempFile, Error> {
    let parent_dir = current_exe.parent().ok_or(Error::NoParentDirectory)?;

    let prefix = if let Some(stem) = current_exe.file_stem() {
        format!(".{}.{}", stem.display(), TEMP_PREFIX)
    } else {
        format!(".{}", TEMP_PREFIX)
    };

    let temp_file = tempfile::Builder::new()
        .prefix(&prefix)
        .permissions(permissions.clone())
        .tempfile_in(parent_dir)
        .map_err(Error::TempCopyFailed)?;

    debug!(
        "copying new binary to temp file: {} -> {}",
        new_bin.display(),
        temp_file.path().display()
    );
    fs::copy(new_bin, temp_file.path()).map_err(Error::TempCopyFailed)?;
    debug!("temp file created at: {}", temp_file.path().display());

    Ok(temp_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn test_create_backup() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_binary");

        let mut file = File::create(&test_file).unwrap();
        file.write_all(b"test content").unwrap();

        let backup_path = create_backup(&test_file).unwrap();

        assert!(backup_path.exists());
        let backup_content = fs::read(&backup_path).unwrap();
        assert_eq!(backup_content, b"test content");
    }

    #[test]
    fn test_create_temp_file_no_collision() {
        let temp_dir = TempDir::new().unwrap();
        let current_exe = temp_dir.path().join("current_binary");
        let new_bin = temp_dir.path().join("new_binary");

        fs::write(&current_exe, b"old").unwrap();
        fs::write(&new_bin, b"new").unwrap();

        // Get default permissions for test
        let permissions = fs::metadata(&current_exe).unwrap().permissions();

        // Create multiple temp files to verify they don't collide
        let temp_file1 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();
        let temp_file2 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();
        let temp_file3 = create_temp_file(&current_exe, &new_bin, &permissions).unwrap();

        // Convert to TempPath to persist them for assertions
        let temp_path1 = temp_file1.into_temp_path();
        let temp_path2 = temp_file2.into_temp_path();
        let temp_path3 = temp_file3.into_temp_path();

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
        let test_file = temp_dir.path().join("test_binary");

        fs::write(&test_file, b"").unwrap();

        let mut perms = fs::metadata(&test_file).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&test_file, perms).unwrap();

        let metadata = fs::metadata(&test_file).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    }
}
