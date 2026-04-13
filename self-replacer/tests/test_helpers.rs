//! Common test utilities shared in integration tests.

use std::fs;
use std::path::{Path, PathBuf};

/// Returns path to the pre-built example binary, building it if necessary.
///
/// Locates the `self_replacing_binary` example by navigating from the test executable's
/// location to the examples directory. If the binary doesn't exist, automatically builds
/// it using `cargo build --example self_replacing_binary`.
///
/// This ensures tests work both locally and in CI without requiring a separate build step.
pub fn get_example_binary() -> PathBuf {
    // Get the target directory (usually target/debug or target/release)
    let mut path = std::env::current_exe().expect("Failed to get current test executable path");

    // Navigate from test binary location to examples directory
    // Test is at: target/debug/deps/integration_tests-<hash>
    // Example is at: target/debug/examples/self_replacing_binary
    path.pop(); // Remove test binary name
    path.pop(); // Remove 'deps' directory
    path.push("examples");

    let binary_name = if cfg!(windows) {
        "self_replacing_binary.exe"
    } else {
        "self_replacing_binary"
    };
    path.push(binary_name);

    // Build the example if it doesn't exist
    if !path.exists() {
        eprintln!("Example binary not found, building it...");
        let output = std::process::Command::new("cargo")
            .arg("build")
            .arg("--example")
            .arg("self_replacing_binary")
            .output()
            .expect("Failed to run cargo build");

        if !output.status.success() {
            panic!(
                "Failed to build example binary:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    assert!(
        path.exists(),
        "Example binary not found at {:?} even after building",
        path
    );

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
