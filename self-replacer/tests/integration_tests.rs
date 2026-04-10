//! Cross-platform integration tests for self-replacer using actual binaries.
//!
//! These tests compile and run real binaries to verify the self-replacement
//! behavior works correctly in realistic scenarios.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

mod test_helpers;
use test_helpers::create_self_replacing_binary;

#[cfg(unix)]
use self_replacer::{SelfReplacer, UnixSelfReplacer};

use self_replacer::BACKUP_SUFFIX;
#[cfg(windows)]
use self_replacer::{SelfReplacer, WindowsSelfReplacer};

// ============================================================================
// Common tests that run on all platforms
// ============================================================================

const TEST_EXEC_MODE: u32 = 0o754; // rwxr-xr--

#[test]
fn test_self_replacement_with_real_binary() {
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path().to_path_buf();

    let binary_v1 = create_self_replacing_binary(&test_dir, "test_app", "1.0.0");
    let binary_v2 = create_self_replacing_binary(&test_dir, "test_app_v2", "2.0.0");

    // Verify v1 prints correct version
    Command::new(&binary_v1)
        .assert()
        .success()
        .stdout(predicate::str::contains("VERSION:1.0.0"));

    // Perform self-replacement
    Command::new(&binary_v1)
        .arg("--replace")
        .arg(&binary_v2)
        .assert()
        .success()
        .stdout(predicate::str::contains("REPLACEMENT_SUCCESS"));

    // Verify the binary was replaced (should now be v2)
    Command::new(&binary_v1)
        .assert()
        .success()
        .stdout(predicate::str::contains("VERSION:2.0.0"));

    // Verify backup was created
    // Backup appends .bak to the full filename (e.g., test_app.exe.bak on Windows)
    let backup_path = {
        let filename = binary_v1.file_name().unwrap();
        let backup_name = format!("{}.{}", filename.to_string_lossy(), BACKUP_SUFFIX);
        binary_v1.with_file_name(backup_name)
    };
    assert!(
        backup_path.exists(),
        "Backup file should exist at {:?}",
        backup_path
    );
}

#[test]
fn test_rollback_on_invalid_path() {
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path().to_path_buf();

    // Create a real binary to be the "current" executable
    let original_binary = create_self_replacing_binary(&test_dir, "test_rollback", "1.0.0");

    // Store original content
    let original_content = fs::read(&original_binary).unwrap();

    // Try to replace with non-existent binary from a separate process
    // We need to simulate this without actually being the running binary
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

    #[test]
    fn test_permission_preservation_with_real_binary() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().to_path_buf();

        // Create binaries
        let binary_v1 = create_self_replacing_binary(&test_dir, "test_perms", "1.0.0");
        let binary_v2 = create_self_replacing_binary(&test_dir, "test_perms_v2", "2.0.0");

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
