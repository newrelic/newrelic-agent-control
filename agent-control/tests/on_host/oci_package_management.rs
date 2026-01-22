use crate::on_host::tools::oci_artifact::REGISTRY_URL;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use crate::on_host::tools::{
    oci_artifact::push_agent_package, oci_package_manager::new_testing_oci_package_manager,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::package::manager::{PackageData, PackageManager};
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use newrelic_agent_control::package::oci::package_manager::get_package_path;
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

    let (_artifact_digest, reference) = push_agent_package(
        &file_to_push,
        REGISTRY_URL,
        PackageMediaType::AgentPackageLayerTarGz,
    );

    let temp_dir = tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    let package_manager = new_testing_oci_package_manager(base_path.clone());

    let agent_id = AgentID::try_from("test-agent").unwrap();
    let pkg_id = "test-package".to_string();

    // Install
    let package_data = PackageData {
        id: pkg_id.clone(),
        oci_reference: reference.clone(),
    };
    let installed_package_result = package_manager.install(&agent_id, package_data);

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
    let expected_path = get_package_path(&base_path, &agent_id, &pkg_id, &reference).unwrap();

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

    let (_artifact_digest, reference) = push_agent_package(
        &file_to_push,
        REGISTRY_URL,
        PackageMediaType::AgentPackageLayerTarGz,
    );

    let temp_dir = tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();
    let package_manager = new_testing_oci_package_manager(base_path.clone());

    let agent_id = AgentID::try_from("test-agent").unwrap();
    let pkg_id = "test-package-idempotency";

    let package_data = PackageData {
        id: pkg_id.to_string(),
        oci_reference: reference.clone(),
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
