use super::super::utils::validate_path;
use super::LocalFile;
use std::path::Path;
use std::{fs, io};
use tracing::instrument;

/// Copies a file on disk, keeping the destination's permissions consistent with the source.
pub trait FileCopier {
    /// Copies the file at `from` to the file at `to`, creating or truncating `to`.
    ///
    /// The copy is byte-for-byte, so non-UTF-8 binaries are preserved exactly, and the destination
    /// keeps the permissions a plain copy yields: on Unix the source's mode bits are carried over
    /// (e.g. the executable bit); on Windows the destination takes the default ACL for its location.
    ///
    /// The destination's parent directory must already exist.
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
}

impl FileCopier for LocalFile {
    /// Copies `from` to `to` byte-for-byte, leaving the destination with the permissions a plain
    /// copy yields (the source's mode on Unix; the default ACL on Windows).
    #[instrument(skip_all, fields(from = %from.display(), to = %to.display()))]
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        validate_path(to)?;

        fs::copy(from, to)?;

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

    /// A copied executable must stay runnable: copy the running test binary and launch the copy.
    /// The earlier implementation dropped `FILE_EXECUTE`, which this would have caught.
    #[cfg(windows)]
    #[test]
    fn test_copied_executable_is_still_executable() {
        use std::process::{Command, Stdio};

        let source = std::env::current_exe().expect("path to the running test executable");
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("copied-test-bin.exe");

        LocalFile.copy(&source, &dst).unwrap();

        // `--list` makes the test harness exit 0 without running any test; a successful launch
        // proves the copy is executable (a missing `FILE_EXECUTE` would fail the spawn instead).
        let status = Command::new(&dst)
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("the copied executable should be launchable");

        assert!(
            status.success(),
            "copied executable failed to run: {status:?}"
        );
    }
}
