//! Windows implementation of [`SelfReplacer`].
//!
//! # Replacement strategy
//!
//! On Windows, the replacement is performed via two metadata-only `rename`
//! operations:
//! 1. Move the currently running binary to a backup location on the same volume.
//! 2. Move the new binary into the now-vacated slot of the original executable
//!    path.
//!
//! Important note: The backup file does not get deleted by this function, and is up to
//! the caller to clean up at a later point.
//!
//! # Why rename instead of a write or copy?
//!
//! When the Windows image loader maps an executable into memory it opens the
//! file with [`FILE_SHARE_DELETE`] but **not** `FILE_SHARE_WRITE`. The
//! `FILE_SHARE_DELETE` flag permits other callers to request delete access on
//! the same handle — and a rename (move) internally requires delete access —
//! so a rename of a running `.exe` succeeds. A direct write or overwrite of
//! the same file would fail with `ERROR_SHARING_VIOLATION` because write
//! access is not shared.
//!
//! Reference: [`CreateFileW` — `dwShareMode` / `FILE_SHARE_DELETE`][msdn-create-file]
//!
//! [msdn-create-file]: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew

use std::fs::rename;
use std::io;
use std::path::{Path, PathBuf};
use tracing::error;

use super::SelfReplacer;

#[derive(Debug, thiserror::Error)]
pub enum ReplaceError {
    #[error("could not determine current executable path: {0}")]
    CurrentExe(#[source] io::Error),

    #[error("new binary does not exist at path: {0}")]
    NewBinMissing(String),

    #[error("failed to move current binary to backup location: {0}")]
    Backup(#[source] io::Error),

    #[error("failed to move new binary into place: {0}")]
    Replace(#[source] io::Error),

    #[error("failed to move new binary into place: {0}; failed to restore backup: {1}")]
    RestoreBackup(#[source] io::Error, io::Error),
}

/// Windows implementation of [`SelfReplacer`].
pub struct WindowsSelfReplacer;

impl SelfReplacer for WindowsSelfReplacer {
    type Error = ReplaceError;

    fn self_replace(&self, new_bin: impl AsRef<Path>) -> Result<(), ReplaceError> {
        let current_exe = std::env::current_exe().map_err(ReplaceError::CurrentExe)?;
        replace_binary(&current_exe, new_bin.as_ref())
    }
}

fn replace_binary(current_exe: &Path, new_bin: &Path) -> Result<(), ReplaceError> {
    let backup = backup_path(current_exe);

    // Rename will replace the backup if it already exists.
    rename(current_exe, &backup).map_err(ReplaceError::Backup)?;

    if let Err(err) = rename(new_bin, current_exe) {
        error!(
            current_exe = %current_exe.display(),
            new_bin = %new_bin.display(),
            %err,
            "Failed to replace binary with new version"
        );
        if let Err(restore_err) = rename(&backup, current_exe) {
            error!(
                backup = %backup.display(),
                %restore_err,
                "Failed to restore backup after self-replace failure"
            );
            return Err(ReplaceError::RestoreBackup(err, restore_err));
        }
        return Err(if err.kind() == io::ErrorKind::NotFound {
            ReplaceError::NewBinMissing(new_bin.display().to_string())
        } else {
            ReplaceError::Replace(err)
        });
    }

    Ok(())
}

fn backup_path(exe_path: &Path) -> PathBuf {
    let filename = exe_path
        .file_name()
        .map(|f| format!("{}.bak", f.to_string_lossy()))
        .unwrap_or_else(|| "replaced_binary.bak".to_string());

    exe_path.with_file_name(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::fs::{read, remove_file, write};
    use std::os::windows::ffi::OsStrExt;
    use tempfile::{TempDir, tempdir};
    use windows::Win32::Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, OPEN_EXISTING,
    };

    #[test]
    fn test_replace_binary_succeeds() {
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
    fn test_replace_binary_overrides_stale_backup_before_replace() {
        let files = TestFiles::new();
        let backup = backup_path(&files.current_exe);
        write(&files.current_exe, b"old binary").unwrap();
        write(&files.new_bin, b"new binary").unwrap();
        write(&backup, b"stale backup from previous run").unwrap();

        let _lock = SimulatedLock::execution_lock(&files.current_exe).unwrap();

        replace_binary(&files.current_exe, &files.new_bin).unwrap();

        assert_eq!(read(&backup).unwrap(), b"old binary");
        assert_eq!(read(&files.current_exe).unwrap(), b"new binary");
    }

    #[test]
    fn test_rollback_when_replacement_failed() {
        let files = TestFiles::new();
        write(&files.current_exe, b"old binary").unwrap();
        write(&files.new_bin, b"new binary").unwrap();

        let _lock = SimulatedLock::delete_lock(&files.new_bin).unwrap();

        let err = replace_binary(&files.current_exe, &files.new_bin).unwrap_err();

        assert_matches!(err, ReplaceError::Replace(_));
        assert_eq!(read(&files.current_exe).unwrap(), b"old binary");
    }

    #[test]
    fn test_replace_fails_if_current_exe_missing() {
        let files = TestFiles::new();
        write(&files.new_bin, b"new binary").unwrap();

        let err = replace_binary(&files.current_exe, &files.new_bin).unwrap_err();

        assert_matches!(err, ReplaceError::Backup(_));
        assert!(!backup_path(&files.current_exe).exists());
    }

    #[test]
    fn test_replace_fails_if_new_bin_missing() {
        let files = TestFiles::new();
        write(&files.current_exe, b"old binary").unwrap();

        let err = replace_binary(&files.current_exe, &files.new_bin).unwrap_err();

        assert_matches!(err, ReplaceError::NewBinMissing(_));
        assert!(files.current_exe.exists());
        assert!(!backup_path(&files.current_exe).exists());
    }

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
    fn test_helper() {
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
