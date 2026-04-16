use std::path::PathBuf;
use tempfile::TempDir;

#[cfg(target_family = "unix")]
const FAKE_AC_BINARY_NAME: &str = "newrelic-agent-control";
#[cfg(target_family = "windows")]
const FAKE_AC_BINARY_NAME: &str = "newrelic-agent-control.exe";

/// Compiles `tests/on_host/data/fake_ac.rs` into a temporary directory and returns
/// both the directory (which must be kept alive) and the path to the binary.
pub fn build_fake_ac_binary() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir for fake binary");
    let binary_path = dir.path().join(FAKE_AC_BINARY_NAME);
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/on_host/data/fake_ac.rs");
    let status = std::process::Command::new("rustc")
        .arg(&src)
        .arg("-o")
        .arg(&binary_path)
        .status()
        .expect("failed to invoke rustc");
    assert!(status.success(), "failed to compile fake_ac.rs");
    (dir, binary_path)
}
