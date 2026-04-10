//! Common test utilities shared in integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the self-replacing binary fixture source
const SELF_REPLACING_BINARY_SOURCE: &str = include_str!("fixtures/self_replacing_binary.rs");

/// Helper to create a binary that uses self_replacer to update itself
pub fn create_self_replacing_binary(dir: &Path, name: &str, version: &str) -> PathBuf {
    let binary_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    let source_file = dir.join(format!("{}.rs", name));
    fs::write(&source_file, SELF_REPLACING_BINARY_SOURCE).unwrap();

    let binary_path = dir.join(&binary_name);

    // Compile with self-replacer dependency
    // Get the self-replacer crate path - it should be the current directory when tests run
    let self_replacer_path = std::env::current_dir().unwrap().canonicalize().unwrap();

    // Convert path to string suitable for TOML
    // On Windows, canonicalize() returns UNC paths like \\?\C:\path which need special handling
    let mut path_str = self_replacer_path.display().to_string();

    // Strip Windows UNC prefix if present
    if path_str.starts_with(r"\\?\") {
        path_str = path_str[4..].to_string();
    }

    // Convert backslashes to forward slashes (works on all platforms)
    let path_str = path_str.replace('\\', "/");

    let manifest = format!(
        r#"
[package]
name = "{}"
version = "{}"
edition = "2024"

[dependencies]
self-replacer = {{ path = "{}" }}
"#,
        name, version, path_str
    );

    let project_dir = dir.join(format!("{}_project", name));
    fs::create_dir_all(project_dir.join("src")).unwrap();

    let manifest_path = project_dir.join("Cargo.toml");
    fs::write(&manifest_path, manifest).unwrap();

    let main_rs = project_dir.join("src/main.rs");
    fs::copy(&source_file, &main_rs).unwrap();

    // Build the binary
    let output = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .output()
        .expect("Failed to build self-replacing binary");

    assert!(
        output.status.success(),
        "Failed to build binary: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let built_binary = project_dir.join("target/release").join(&binary_name);
    fs::copy(&built_binary, &binary_path).unwrap();

    // Make it executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms).unwrap();
    }

    binary_path
}
