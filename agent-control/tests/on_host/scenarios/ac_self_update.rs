use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::oci_signer::OCISigner;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_artifact::push_agent_package;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use std::fs;
use std::time::Duration;
use tempfile::tempdir;

const AC_VERSION_2: &str = "2.0.0";

#[cfg(not(target_os = "windows"))]
const AC_BINARY_NAME: &str = "newrelic-agent-control";
#[cfg(target_os = "windows")]
const AC_BINARY_NAME: &str = "newrelic-agent-control.exe";

/// Creates a fake agent-control binary that outputs its version when called with `verify`
#[cfg(not(target_os = "windows"))]
fn create_fake_ac_binary(version: &str) -> String {
    format!(
        r#"#!/bin/bash
if [[ "$1" == "verify" ]]; then
    echo '{{"message": "verification successful"}}'
    exit 0
fi
echo "{version}"
sleep 60
"#
    )
}

#[cfg(target_os = "windows")]
fn create_fake_ac_binary(version: &str) -> String {
    format!(
        r#"param ([switch]$Verify)
if ($Verify) {{
    Write-Host '{{"message": "verification successful"}}'
    exit 0
}}
Write-Host "{version}"
Start-Sleep -Seconds 60
"#
    )
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), needs elevated privileges"]
fn test_agent_control_self_update_with_oci_registry_with_oci_registry() {
    let signer = OCISigner::start();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    // Push fake AC binary package to OCI registry
    let version_2_ref = push_fake_ac_package(AC_VERSION_2, &signer);

    let mut opamp_server = FakeServer::start_new();

    // Create initial config with ac_remote_update enabled and package configuration
    let initial_config = format!(
        r#"
agents: {{}}
server:
  enabled: true
  ac_remote_update: true
  packages:
    binary:
      download:
        oci:
          registry: {OCI_TEST_REGISTRY_URL}
          repository: test
          public_key_url: {}
"#,
        signer.jwks_url()
    );

    let config_path = local_dir
        .path()
        .join("newrelic-agent-control")
        .join("store")
        .join("config.yaml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, initial_config).unwrap();

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    // Verify AC is running with initial version
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });

    // Send remote config to trigger update to version 2
    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        version_2_ref
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Verify AC remains healthy during the update process
    // Note: After self-replacement, AC will stop and would need an external
    // controller (like systemd) to restart with the new binary. In this test,
    // we verify the update process was initiated and AC handled it gracefully.
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });

    // Give AC time to process the update, download, verify, and stage the replacement
    std::thread::sleep(Duration::from_secs(10));

    // Verify the new binary was downloaded and placed in the correct location
    let new_binary_path = remote_dir
        .path()
        .join("newrelic-agent-control")
        .join("packages")
        .join("binary")
        .join("installation")
        .join(AC_BINARY_NAME);

    assert!(
        new_binary_path.exists(),
        "New binary should be downloaded and extracted"
    );

    // Verify the downloaded binary responds correctly to verify command
    let verify_result = std::process::Command::new(&new_binary_path)
        .arg("verify")
        .output()
        .expect("Failed to execute verify command");

    assert!(
        verify_result.status.success(),
        "Verify command should succeed on downloaded binary"
    );
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix), needs elevated privileges"]
fn test_agent_control_self_update_with_unsigned_package_fails_with_oci_registry() {
    let signer = OCISigner::start();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    // Push unsigned package (no signer provided)
    let unsigned_version_ref = push_fake_ac_package_unsigned(AC_VERSION_2);

    let mut opamp_server = FakeServer::start_new();

    let initial_config = format!(
        r#"
agents: {{}}
server:
  enabled: true
  ac_remote_update: true
  packages:
    binary:
      download:
        oci:
          registry: {OCI_TEST_REGISTRY_URL}
          repository: test
          public_key_url: {}
"#,
        signer.jwks_url()
    );

    let config_path = local_dir
        .path()
        .join("newrelic-agent-control")
        .join("store")
        .join("config.yaml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, initial_config).unwrap();

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    // Wait for AC to start
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });

    // Send remote config with unsigned package version
    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        unsigned_version_ref
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Give AC time to attempt the update
    std::thread::sleep(Duration::from_secs(15));

    // Verify update fails due to signature verification - the new binary should NOT exist
    let new_binary_path = remote_dir
        .path()
        .join("newrelic-agent-control")
        .join("packages")
        .join("binary")
        .join("installation")
        .join(AC_BINARY_NAME);

    assert!(
        !new_binary_path.exists(),
        "Unsigned binary should not be installed due to signature verification failure"
    );

    // Verify AC is still healthy (didn't crash from the failed update)
    retry(10, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &ac_instance_id)?;
        Ok(())
    });
}

/// Pushes a fake agent-control binary package to OCI registry and signs it
fn push_fake_ac_package(version: &str, signer: &OCISigner) -> String {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();

    #[cfg(not(target_os = "windows"))]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.tar.gz");
        TestDataHelper::compress_tar_gz(
            tmp_dir_to_compress.path(),
            &path,
            &create_fake_ac_binary(version),
            AC_BINARY_NAME,
        );
        (path, PackageMediaType::AgentPackageLayerTarGz)
    };

    #[cfg(target_os = "windows")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.zip");
        TestDataHelper::compress_zip(
            tmp_dir_to_compress.path(),
            &path,
            &create_fake_ac_binary(version),
            AC_BINARY_NAME,
        );
        (path, PackageMediaType::AgentPackageLayerZip)
    };

    let (_, reference) = push_agent_package(&path, OCI_TEST_REGISTRY_URL, media_type);

    signer.sign_artifact(&reference);

    reference.tag().unwrap().to_string()
}

/// Pushes an unsigned fake agent-control binary package
fn push_fake_ac_package_unsigned(version: &str) -> String {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();

    #[cfg(not(target_os = "windows"))]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.tar.gz");
        TestDataHelper::compress_tar_gz(
            tmp_dir_to_compress.path(),
            &path,
            &create_fake_ac_binary(version),
            AC_BINARY_NAME,
        );
        (path, PackageMediaType::AgentPackageLayerTarGz)
    };

    #[cfg(target_os = "windows")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.zip");
        TestDataHelper::compress_zip(
            tmp_dir_to_compress.path(),
            &path,
            &create_fake_ac_binary(version),
            AC_BINARY_NAME,
        );
        (path, PackageMediaType::AgentPackageLayerZip)
    };

    let (_, reference) = push_agent_package(&path, OCI_TEST_REGISTRY_URL, media_type);

    reference.tag().unwrap().to_string()
}
