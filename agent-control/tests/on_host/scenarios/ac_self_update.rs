use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::oci_signer::OCISigner;
use crate::common::opamp::FakeServer;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::retry;
use crate::on_host::tools::config::create_local_config;
use crate::on_host::tools::fake_binary::assert_current_exe_is_fake_ac;
use crate::on_host::tools::fake_binary::build_fake_ac_binary;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_artifact::push_agent_package;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_ID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
/// Be aware that this test executes a binary self-replace which in this case is the test binary itself.
fn test_agent_control_self_update_with_oci_registry() {
    let signer = OCISigner::start();
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let new_version_tag = push_fake_ac_package(&signer);

    let agent_control_config = format!(
        r#"
host_id: integration-test
fleet_control:
  endpoint: {}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {}
agents: {{}}
self_update:
  enabled: true
  signature_verification_enabled: true
  package:
    download:
      oci:
        registry: {OCI_TEST_REGISTRY_URL}
        repository: test
        public_key_url: {}
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        signer.jwks_url()
    );
    create_local_config(
        AGENT_CONTROL_ID,
        agent_control_config,
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let mut agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        new_version_tag
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Expect a applied remote config in case the updater was successful executed.
    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )
    });
    // The updater should trigger the graceful stop of AC.
    retry(60, Duration::from_secs(5), || {
        if agent_control.has_gracefully_stopped() {
            Ok(())
        } else {
            Err("Agent Control should have stopped for the new binary to take effect".into())
        }
    });

    assert_current_exe_is_fake_ac();
}

/// Pushes a fake agent-control binary package to OCI registry and signs it
fn push_fake_ac_package(signer: &OCISigner) -> String {
    let dir = tempdir().unwrap();

    let (_binary_dir, binary_path) = build_fake_ac_binary();

    #[cfg(target_family = "unix")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.tar.gz");
        TestDataHelper::compress_tar_gz_file(&binary_path, &path);
        (path, PackageMediaType::AgentPackageLayerTarGz)
    };

    #[cfg(target_family = "windows")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.zip");
        TestDataHelper::compress_zip_file(&binary_path, &path);
        (path, PackageMediaType::AgentPackageLayerZip)
    };

    let (_, reference) = push_agent_package(&path, OCI_TEST_REGISTRY_URL, media_type);

    signer.sign_artifact(&reference);

    reference.tag().unwrap().to_string()
}
