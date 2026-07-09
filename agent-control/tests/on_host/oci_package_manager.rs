use std::collections::HashMap;
use std::str::FromStr;

use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_package_manager::{TestDataHelper, new_testing_oci_package_manager};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::agent_type::runtime_config::on_host::executable::rendered::{
    Args, Env,
};
use newrelic_agent_control::agent_type::runtime_config::on_host::package::rendered::{
    Oci, PostDownloadHook, Repository, Version,
};
use newrelic_agent_control::package::manager::{PackageData, PackageManager};
use newrelic_agent_control::package::oci::package_manager::get_package_path;
use oci_test_utils::{PackageMediaType, PackagePublisher};
use tempfile::tempdir;

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), needs elevated privileges on Windows"]
fn test_install_and_uninstall_with_oci_registry() {
    const FILENAME: &str = "file1.txt";
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();
    let file_to_push = dir.path().join("layer_digest.tar.gz");

    TestDataHelper::compress_tar_gz(
        tmp_dir_to_compress.path(),
        file_to_push.as_path(),
        "important content",
        FILENAME,
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&file_to_push, PackageMediaType::TarGz);

    let temp_dir = tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    let package_manager =
        new_testing_oci_package_manager(base_path.clone(), OCI_TEST_REGISTRY_URL.to_string());

    let agent_id = AgentID::try_from("test-agent").unwrap();
    let pkg_id = "test-package".to_string();

    // Install
    let package_data = PackageData {
        id: pkg_id.clone(),
        oci: Oci {
            repository: Repository::from_str(reference.repository()).unwrap(),
            version: Version::from_str(reference.tag().unwrap()).unwrap(),
            public_key_url: None,
        },
        post_download_hook: None,
    };
    let installed_package_result = package_manager.install(&agent_id, package_data.clone());

    assert!(
        installed_package_result.is_ok(),
        "Installation failed: {:?}",
        installed_package_result.as_ref().unwrap_err()
    );

    let installed_package = installed_package_result.unwrap();
    TestDataHelper::test_tar_gz_uncompressed(
        installed_package.installation_path.as_path(),
        FILENAME,
    );
    // Verify location
    // The path should be base_path/agent_id/oci_registry__port__repo_tag
    let expected_path = get_package_path(&base_path, &agent_id, &package_data).unwrap();

    assert_eq!(installed_package.installation_path, expected_path);

    // Uninstall
    let installation_path = installed_package.installation_path.clone();
    package_manager
        .uninstall(&agent_id, installed_package)
        .unwrap();
    assert!(!installation_path.exists());
}

#[test]
#[ignore = "needs oci registry, needs elevated privileges on Windows"]
fn test_install_skips_download_if_exists_with_oci_registry() {
    const FILENAME: &str = "payload.txt";

    let dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    let file_to_push = dir.path().join("layer_digest.tar.gz");

    TestDataHelper::compress_tar_gz(
        content_dir.path(),
        file_to_push.as_path(),
        "ORIGINAL_CONTENT",
        FILENAME,
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&file_to_push, PackageMediaType::TarGz);

    let temp_dir = tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();
    let package_manager =
        new_testing_oci_package_manager(base_path.clone(), OCI_TEST_REGISTRY_URL.to_string());

    let agent_id = AgentID::try_from("test-agent").unwrap();
    let pkg_id = "test-package-idempotency";

    let package_data = PackageData {
        id: pkg_id.to_string(),
        oci: Oci {
            repository: Repository::from_str(reference.repository()).unwrap(),
            version: Version::from_str(reference.tag().unwrap()).unwrap(),
            public_key_url: None,
        },
        post_download_hook: None,
    };

    let installed_1 = package_manager
        .install(&agent_id, package_data.clone())
        .expect("First install failed");

    let installed_file_path = installed_1.installation_path.join(FILENAME);
    assert!(
        installed_file_path.exists(),
        "Payload file should exist after install"
    );

    let content_1 = std::fs::read_to_string(&installed_file_path).expect("Failed to read payload");
    assert_eq!(content_1, "ORIGINAL_CONTENT");

    std::fs::write(&installed_file_path, "MODIFIED_CONTENT_BY_USER").unwrap();

    let result_2 = package_manager.install(&agent_id, package_data);
    assert!(result_2.is_ok());

    let content_2 = std::fs::read_to_string(&installed_file_path).unwrap();

    assert_eq!(
        content_2, "MODIFIED_CONTENT_BY_USER",
        "The package manager overwrote the existing files! It should have skipped download/extraction."
    );
}

// The hook must run on every install, even when the package is already on disk (AC restart or
// rollback). It appends one line per run, so two installs of the same package give two lines.
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_install_runs_hook_on_every_install_even_when_present_with_oci_registry() {
    const FILENAME: &str = "payload.txt";

    let dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();
    let file_to_push = dir.path().join("layer_digest.tar.gz");

    TestDataHelper::compress_tar_gz(
        content_dir.path(),
        file_to_push.as_path(),
        "PAYLOAD",
        FILENAME,
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&file_to_push, PackageMediaType::TarGz);

    // Cross-platform hook: the test binary invokes the ignored `post_download_hook_probe` below,
    // which appends a line to `NR_HOOK_COUNTER_FILE` per run (works on Windows too, no shell script).
    let hook_dir = tempdir().unwrap();
    let counter_path = hook_dir.path().join("hook-runs.count");
    let test_bin = std::env::current_exe().expect("path to the running test executable");

    let temp_dir = tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();
    let package_manager =
        new_testing_oci_package_manager(base_path.clone(), OCI_TEST_REGISTRY_URL.to_string());

    let agent_id = AgentID::try_from("test-agent").unwrap();

    let package_data = PackageData {
        id: "test-package-hook-every-install".to_string(),
        oci: Oci {
            repository: Repository::from_str(reference.repository()).unwrap(),
            version: Version::from_str(reference.tag().unwrap()).unwrap(),
            public_key_url: None,
        },
        post_download_hook: Some(PostDownloadHook {
            path: test_bin.to_string_lossy().to_string(),
            // Filter to the probe by name, plus `--ignored`.
            args: Args(vec![
                "post_download_hook_probe".to_string(),
                "--ignored".to_string(),
            ]),
            env: Env(HashMap::from([(
                "NR_HOOK_COUNTER_FILE".to_string(),
                counter_path.to_string_lossy().to_string(),
            )])),
        }),
    };

    let count_lines = || {
        std::fs::read_to_string(&counter_path)
            .map(|c| c.lines().count())
            .unwrap_or(0)
    };

    package_manager
        .install(&agent_id, package_data.clone())
        .expect("first install failed");
    assert_eq!(
        count_lines(),
        1,
        "hook should have run once after first install"
    );

    package_manager
        .install(&agent_id, package_data)
        .expect("second install failed");
    assert_eq!(
        count_lines(),
        2,
        "hook should run again on the second install even though the package is already present"
    );
}

// Not a real test: invoked as the post-download hook (via the test binary, so it's cross-platform).
// Appends one line to `NR_HOOK_COUNTER_FILE` per run so the caller can count hook executions; no-op
// if the var is unset.
#[test]
#[ignore = "helper invoked as a post-download hook, not a standalone test"]
fn post_download_hook_probe() {
    use std::io::Write;

    let Ok(counter_path) = std::env::var("NR_HOOK_COUNTER_FILE") else {
        return;
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(counter_path)
        .expect("open hook counter file");
    writeln!(file, "run").expect("append to hook counter file");
}
