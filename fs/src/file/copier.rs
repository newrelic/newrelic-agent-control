use super::super::utils::validate_path;
use super::LocalFile;
use std::path::Path;
use std::{fs, io};
use tracing::instrument;

/// Copies a file on disk, preserving the source's permissions on Unix.
pub trait FileCopier {
    /// Copies the file at `from` to the file at `to`, creating or truncating `to`.
    ///
    /// The copy is byte-for-byte, so non-UTF-8 binaries are preserved exactly. On Unix it
    /// **preserves the source file's mode bits**.
    ///
    /// On Windows, permissions cannot be preserved this way (the copy does not carry the source's
    /// ACL, and executability is determined by extension, not permissions), so the destination is
    /// restricted to administrators, matching [super::writer::FileWriter].
    ///
    /// The destination's parent directory must already exist.
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
}

impl FileCopier for LocalFile {
    /// Copies `from` to `to` byte-for-byte, preserving the source's mode on Unix and applying the
    /// administrators-only ACL on Windows.
    #[instrument(skip_all, fields(from = %from.display(), to = %to.display()))]
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        validate_path(to)?;

        fs::copy(from, to)?;

        #[cfg(target_family = "windows")]
        {
            crate::win_permissions::set_file_permissions_for_administrator(to)
                .map_err(io::Error::other)?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(missing_docs)] // test-support code
pub mod tests {
    use super::*;

    /// A byte-for-byte copy preserves non-UTF-8 content and the source file's mode (whatever it is,
    /// executable or not) rather than forcing a fixed mode.
    #[cfg(unix)]
    #[test]
    fn test_copy_preserves_non_utf8_bytes_and_source_mode() {
        use std::os::unix::fs::PermissionsExt;

        let bytes = [0xFFu8, 0xFE, 0x00, 0x01, b'h', b'i'];

        // The low 9 bits of a Unix mode (rwxrwxrwx); the rest of `st_mode` holds the file-type and
        // setuid/setgid/sticky bits, which we don't compare here.
        const PERMISSION_BITS: u32 = 0o777;

        // Both an executable and a non-executable source mode must be reproduced on the copy.
        let source_modes: [u32; 2] = [0o644, 0o755];
        for mode in source_modes {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("bin");
            let dst = dir.path().join("copied-bin");

            fs::write(&src, bytes).unwrap();
            fs::set_permissions(&src, fs::Permissions::from_mode(mode)).unwrap();

            LocalFile.copy(&src, &dst).unwrap();

            let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & PERMISSION_BITS;
            assert_eq!(fs::read(&dst).unwrap(), bytes, "content must be preserved");
            assert_eq!(
                dst_mode, mode,
                "destination must preserve the source's mode {mode:#o}"
            );
        }
    }

    /// Copying onto an existing file replaces the existing
    #[cfg(unix)]
    #[test]
    fn test_copy_overwrites_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::write(&src, "new").unwrap();
        fs::write(&dst, "older content with greater len").unwrap();

        LocalFile.copy(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
    }

    /// A missing source surfaces an error (and never creates the destination).
    #[test]
    fn test_copy_missing_source_errors() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("dst");

        let result = LocalFile.copy(&dir.path().join("does-not-exist"), &dst);

        assert!(result.is_err());
        assert!(!dst.exists());
    }

    /// The destination path is validated before any I/O: `..` components are rejected.
    #[test]
    fn test_copy_rejects_parent_dir_in_destination() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::write(&src, "data").unwrap();

        let result = LocalFile.copy(&src, Path::new("some/path/../../etc/passwd"));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("dots disallowed in path")
        );
    }

    // Needs administrator rights on Windows (matches the writer tests). On GitHub Actions the
    // Windows runner is elevated, so it passes there.
    #[test]
    #[ignore = "requires windows administrator"]
    #[cfg(target_family = "windows")]
    fn test_copy_sets_windows_admin_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::write(&src, "data").unwrap();

        LocalFile.copy(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "data");
        crate::win_permissions::tests::assert_windows_permissions(&dst);
    }
}
