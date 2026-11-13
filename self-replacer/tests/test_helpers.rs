//! Common test utilities shared in integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Returns path to the `self_replacing_binary` example, building it if needed.
/// The result is cached so cargo is invoked once per test process.
pub fn get_example_binary() -> PathBuf {
    static EXAMPLE_BINARY: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_BINARY.get_or_init(build_example_binary).clone()
}

fn build_example_binary() -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--example",
            "self_replacing_binary",
            "--message-format=json",
        ])
        .output()
        .expect("Failed to run cargo build");

    assert!(
        output.status.success(),
        "Failed to build example binary:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|msg| {
            msg["reason"] == "compiler-artifact" && msg["target"]["name"] == "self_replacing_binary"
        })
        .find_map(|msg| msg["executable"].as_str().map(PathBuf::from))
        .expect("cargo did not report the example executable");

    assert!(path.exists(), "Example binary not found at {path:?}");
    path
}

/// Creates a copy of the example binary at the specified location.
pub fn copy_example_binary(dest: &Path) -> PathBuf {
    let source = get_example_binary();
    fs::copy(&source, dest).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms).unwrap();
    }

    dest.to_path_buf()
}

/// Creates a modified copy of the example binary with different content (different hash).
/// The binary remains functional but will have a different hash when run.
pub fn create_modified_binary(dir: &Path, name: &str) -> PathBuf {
    let source = get_example_binary();

    let dest_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    let dest = dir.join(dest_name);

    // Copy binary and append a null byte to change the hash
    // The binary still works because the OS ignores trailing data
    let mut content = fs::read(&source).unwrap();
    content.push(0);
    fs::write(&dest, content).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms).unwrap();
    }

    dest
}
