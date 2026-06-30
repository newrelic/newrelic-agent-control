use super::super::utils::validate_path;
use super::LocalFile;
use std::path::Path;
use std::{fs, io};
use tracing::instrument;

/// Copies a file on disk, setting permissions on the destination.
pub trait FileCopier {
    /// Copies the file at `from` to the file at `to`, creating or truncating `to`.
    ///
    /// The copy is byte-for-byte, so non-UTF-8 binaries are preserved exactly. On Unix the
    /// destination's permissions are set to `mode` (for example `0o755` for an executable),
    /// overriding whatever the source carried. On Windows `mode` is ignored and the destination
    /// is restricted to administrators, matching [`FileWriter::write`](super::writer::FileWriter).
    ///
    /// `mode` is kept in the signature on every platform on purpose: it expresses the caller's
    /// Unix permission intent (Linux is the primary on-host target), and a single cross-platform
    /// signature lets callers state that intent uniformly. Windows uses an ACL-based model with no
    /// Unix mode to honor, so the value is intentionally ignored there rather than dropped from the
    /// API.
    ///
    /// The destination's parent directory must already exist.
    fn copy(&self, from: &Path, to: &Path, mode: u32) -> io::Result<()>;
}

impl FileCopier for LocalFile {
    /// Copies `from` to `to` byte-for-byte and sets the destination's permissions.
    /// On Unix the destination mode is set to `mode`; on Windows access is restricted to
    /// administrators and `mode` is ignored.
    #[instrument(skip_all, fields(from = %from.display(), to = %to.display()))]
    fn copy(&self, from: &Path, to: &Path, mode: u32) -> io::Result<()> {
        validate_path(to)?;

        fs::copy(from, to)?;

        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(to, fs::Permissions::from_mode(mode))?;
        }

        #[cfg(target_family = "windows")]
        {
            // Windows has no Unix mode to apply: file access is governed by ACLs, which we set the
            // same way `FileWriter::write` does. `mode` is accepted for a uniform cross-platform
            // signature but has no Windows equivalent, so it is intentionally unused here.
            let _ = mode;
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

    /// A byte-for-byte copy preserves non-UTF-8 content and sets the requested mode, regardless of
    /// the source file's own (non-executable) permissions.
    #[cfg(unix)]
    #[test]
    fn test_copy_preserves_non_utf8_bytes_and_overrides_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("bin");
        let dst = dir.path().join("copied-bin");

        let bytes = [0xFFu8, 0xFE, 0x00, 0x01, b'h', b'i'];
        fs::write(&src, bytes).unwrap();
        // Source is deliberately non-executable, to prove we don't inherit its mode.
        fs::set_permissions(&src, fs::Permissions::from_mode(0o600)).unwrap();

        LocalFile.copy(&src, &dst, 0o755).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), bytes, "content must be preserved");
        assert_eq!(
            fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
            0o755,
            "destination mode must be the requested one, not the source's"
        );
    }

    /// Copying onto an existing file.
    #[cfg(unix)]
    #[test]
    fn test_copy_overwrites_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::write(&src, "new").unwrap();
        fs::write(&dst, "older content with greater len").unwrap();

        LocalFile.copy(&src, &dst, 0o644).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
    }

    /// A missing source surfaces an error (and never creates the destination).
    #[test]
    fn test_copy_missing_source_errors() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("dst");

        let result = LocalFile.copy(&dir.path().join("does-not-exist"), &dst, 0o755);

        assert!(result.is_err());
        assert!(!dst.exists());
    }

    /// The destination path is validated before any I/O: `..` components are rejected.
    #[test]
    fn test_copy_rejects_parent_dir_in_destination() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::write(&src, "data").unwrap();

        let result = LocalFile.copy(&src, Path::new("some/path/../../etc/passwd"), 0o755);

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

        LocalFile.copy(&src, &dst, 0o755).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "data");
        crate::win_permissions::tests::assert_windows_permissions(&dst);
    }
}
