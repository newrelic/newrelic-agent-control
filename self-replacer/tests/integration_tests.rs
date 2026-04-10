//! Cross-platform integration tests for self-replacer using actual binaries.
//!
//! These tests compile and run real binaries to verify the self-replacement
//! behavior works correctly in realistic scenarios.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

mod test_helpers;
use test_helpers::{copy_example_binary, create_modified_binary};

#[cfg(unix)]
use self_replacer::{SelfReplacer, UnixSelfReplacer};

use self_replacer::BACKUP_SUFFIX;
#[cfg(windows)]
use self_replacer::{SelfReplacer, WindowsSelfReplacer};

// ============================================================================
// Common tests that run on all platforms
// ============================================================================

#[test]
fn test_self_replacement_with_real_binary() {
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path();

    // Create test binary (copy of example)
    let binary_path = if cfg!(windows) {
        test_dir.join("test_app.exe")
    } else {
        test_dir.join("test_app")
    };
    copy_example_binary(&binary_path);

    // Get original hash
    let output1 = Command::new(&binary_path)
        .output()
        .expect("Failed to get original hash");
    let hash1 = String::from_utf8_lossy(&output1.stdout);
    assert!(hash1.starts_with("HASH:"), "Expected hash output");

    // Create modified binary (different hash)
    let binary_v2 = create_modified_binary(test_dir, "test_app_v2");

    // Perform self-replacement
    Command::new(&binary_path)
        .arg("--replace")
        .arg(&binary_v2)
        .assert()
        .success()
        .stdout(predicate::str::contains("REPLACEMENT_SUCCESS"));

    // Verify the binary was replaced (hash should be different)
    Command::new(&binary_path)
        .assert()
        .success()
        .stdout(predicate::str::starts_with("HASH:"));

    let output2 = Command::new(&binary_path)
        .output()
        .expect("Failed to get new hash");
    let hash2 = String::from_utf8_lossy(&output2.stdout);

    assert_ne!(
        hash1.trim(),
        hash2.trim(),
        "Hash should change after replacement"
    );

    // Verify backup was created
    let backup_path = {
        let filename = binary_path.file_name().unwrap();
        let backup_name = format!("{}.{}", filename.to_string_lossy(), BACKUP_SUFFIX);
        binary_path.with_file_name(backup_name)
    };
    assert!(
        backup_path.exists(),
        "Backup file should exist at {:?}",
        backup_path
    );

    // Verify backup has the original hash
    Command::new(&backup_path)
        .assert()
        .success()
        .stdout(predicate::str::contains(hash1.trim()));
}

#[test]
fn test_rollback_on_invalid_path() {
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path();

    // Create a real binary to be the "current" executable
    let original_binary = if cfg!(windows) {
        test_dir.join("test_rollback.exe")
    } else {
        test_dir.join("test_rollback")
    };
    copy_example_binary(&original_binary);

    // Store original content
    let original_content = fs::read(&original_binary).unwrap();

    // Try to replace with non-existent binary
    let non_existent = if cfg!(windows) {
        test_dir.join("does_not_exist.exe")
    } else {
        test_dir.join("does_not_exist")
    };

    #[cfg(unix)]
    let result = UnixSelfReplacer::self_replace(&non_existent);

    #[cfg(windows)]
    let result = WindowsSelfReplacer::self_replace(&non_existent);

    assert!(result.is_err(), "Should fail when new binary doesn't exist");

    // Verify original binary is unchanged
    let current_content = fs::read(&original_binary).unwrap();
    assert_eq!(
        original_content, current_content,
        "Original binary should be unchanged after failed replacement"
    );
}

// ============================================================================
// Unix-specific tests
// ============================================================================

#[cfg(unix)]
mod unix_specific {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const TEST_EXEC_MODE: u32 = 0o754; // rwxr-xr--

    #[test]
    fn test_permission_preservation_with_real_binary() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path();

        // Create binaries
        let binary_v1 = test_dir.join("test_perms");
        copy_example_binary(&binary_v1);

        let binary_v2 = create_modified_binary(test_dir, "test_perms_v2");

        // Set specific permissions on v1
        let mut perms = fs::metadata(&binary_v1).unwrap().permissions();
        perms.set_mode(TEST_EXEC_MODE);
        fs::set_permissions(&binary_v1, perms).unwrap();

        // The bitmask 0o777 is needed to extract only the 9 permission bits (rwxrwxrwx) and ignore
        // the file type bits. This is the standard practice when comparing Unix file permissions.
        let original_mode = fs::metadata(&binary_v1).unwrap().permissions().mode() & 0o777;
        assert_eq!(original_mode, TEST_EXEC_MODE);

        // Perform replacement
        Command::new(&binary_v1)
            .arg("--replace")
            .arg(&binary_v2)
            .assert()
            .success();

        // Verify permissions were preserved
        let new_mode = fs::metadata(&binary_v1).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            new_mode, original_mode,
            "Permissions should be preserved after replacement"
        );
    }
}
