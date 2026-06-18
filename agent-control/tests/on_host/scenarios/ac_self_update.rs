use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::remote_config_status::check_latest_remote_config_status;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::retry;
use crate::common::retry::retry_never;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::base_paths::TempBasePaths;
use crate::on_host::tools::config::AgentControlConfigBuilder;
use crate::on_host::tools::fake_binary::assert_is_fake_binary;
use crate::on_host::tools::fake_binary::build_fake_ac_binary;
use crate::on_host::tools::fake_binary::build_invalid_fake_ac_binary;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_VERSION;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use oci_test_utils::OCISigner;
use oci_test_utils::{PackageMediaType, PackagePublisher};
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::{TempDir, tempdir};

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
/// This test exercises AC self-update using a fake new version of the AC binary pushed to a local OCI registry.
/// It verifies that AC can fetch the new version from the registry, validate its signature, and apply the update
/// by gracefully stopping itself (the new version would take effect on restart, which is outside the scope of this test).
/// The replaced binary in this case is the compiled test binary, so any other test that successfully executes another self-replacement
/// should be executed sequentially.
fn test_ac_self_update_with_oci_registry() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let new_version_tag = push_signed_fake_ac_package(&signer);

    let dirs = TempBasePaths::new();

    create_self_update_local_config(&opamp_server, &signer, &dirs.local_dir(), true);

    let current_exe_path = std::env::current_exe()
        .expect("failed to get current exe path")
        .canonicalize()
        .expect("failed to canonicalize current exe path");

    let mut agent_control = start_agent_control_with_custom_config(
        dirs.base_paths().clone(),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        new_version_tag
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Expect an applied remote config in case the updater was successfully executed.
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

    assert_is_fake_binary(&current_exe_path);
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_ac_self_update_fails_for_unsigned_package_with_oci_registry() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let new_version_tag = push_unsigned_fake_ac_package();

    let dirs = TempBasePaths::new();

    create_self_update_local_config(&opamp_server, &signer, &dirs.local_dir(), true);

    let mut agent_control = start_agent_control_with_custom_config(
        dirs.base_paths().clone(),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        new_version_tag
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Signature verification must reject the package and report a failed config status
    // with a message mentioning the root cause.
    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status(&opamp_server, &ac_instance_id, |status| {
            if status.status != RemoteConfigStatuses::Failed as i32 {
                return Err(format!("expected Failed status, got: {}", status.status).into());
            }
            if !status.error_message.contains("signature verification") {
                return Err(format!(
                    "expected error message to contain 'signature verification', got: {}",
                    status.error_message
                )
                .into());
            }
            Ok(())
        })
    });

    retry_never(10, Duration::from_secs(1), || {
        if agent_control.has_gracefully_stopped() {
            Err(
                "Agent Control should not have stopped when the package signature is missing"
                    .into(),
            )
        } else {
            Ok(())
        }
    });
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_ac_self_update_does_nothing_for_same_version_with_oci_registry() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let dirs = TempBasePaths::new();

    create_self_update_local_config(&opamp_server, &signer, &dirs.local_dir(), true);

    let mut agent_control = start_agent_control_with_custom_config(
        dirs.base_paths().clone(),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    // Requesting the same version that is already running — AC skips the update without
    // contacting the OCI registry.
    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        AGENT_CONTROL_VERSION
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )
    });

    retry_never(10, Duration::from_secs(1), || {
        if agent_control.has_gracefully_stopped() {
            Err("Agent Control should not have stopped when the requested version is the current one".into())
        } else {
            Ok(())
        }
    });
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_ac_self_update_fails_for_missing_version_with_oci_registry() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let dirs = TempBasePaths::new();

    // Disables signature verification to make sure the test reaches the package fetch step, which should fail for a non-existent version.
    create_self_update_local_config(&opamp_server, &signer, &dirs.local_dir(), false);

    let mut agent_control = start_agent_control_with_custom_config(
        dirs.base_paths().clone(),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths().clone());

    // This tag does not exist in the registry — the package fetch will fail.
    let update_config = r#"
version: "nonexistent-version-tag"
agents: {}
"#;
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status(&opamp_server, &ac_instance_id, |status| {
            if status.status != RemoteConfigStatuses::Failed as i32 {
                return Err(format!("expected Failed status, got: {}", status.status).into());
            }
            if !status
                .error_message
                .contains("requested version does not exist")
            {
                return Err(format!(
                    "expected error message to contain 'requested version does not exist', got: {}",
                    status.error_message
                )
                .into());
            }
            Ok(())
        })
    });

    retry_never(10, Duration::from_secs(1), || {
        if agent_control.has_gracefully_stopped() {
            Err(
                "Agent Control should not have stopped when the requested version does not exist"
                    .into(),
            )
        } else {
            Ok(())
        }
    });
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_ac_self_update_fails_when_binary_verification_fails_with_oci_registry() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let dirs = TempBasePaths::new();

    let new_version_tag = push_signed_invalid_fake_ac_package(&signer);

    create_self_update_local_config(&opamp_server, &signer, &dirs.local_dir(), true);

    let mut agent_control = start_agent_control_with_custom_config(
        dirs.base_paths().clone(),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths().clone());

    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        new_version_tag
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // Binary verify returns exit 1 with a message — expect Failed status with that message.
    retry(60, Duration::from_secs(5), || {
        check_latest_remote_config_status(&opamp_server, &ac_instance_id, |status| {
            if status.status != RemoteConfigStatuses::Failed as i32 {
                return Err(format!("expected Failed status, got: {}", status.status).into());
            }
            if !status.error_message.contains("pre-flight check failed") {
                return Err(format!(
                    "expected error message to contain 'pre-flight check failed', got: {}",
                    status.error_message
                )
                .into());
            }
            Ok(())
        })
    });

    retry_never(10, Duration::from_secs(1), || {
        if agent_control.has_gracefully_stopped() {
            Err("Agent Control should not have stopped when binary verification fails".into())
        } else {
            Ok(())
        }
    });
}

fn create_self_update_local_config(
    opamp_server: &FakeServer,
    signer: &OCISigner,
    local_dir: &Path,
    signature_verification_enabled: bool,
) {
    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_oci_registry(OCI_TEST_REGISTRY_URL)
        .with_self_update(
            signature_verification_enabled,
            "test",
            signer.jwks_url().to_string(),
        )
        .write(local_dir.to_path_buf());
}

/// Pushes an invalid fake agent-control binary package to the OCI registry and signs it.
/// The binary will fail verification (`verify` exits 1 with a structured message).
fn push_signed_invalid_fake_ac_package(signer: &OCISigner) -> String {
    push_ac_package(build_invalid_fake_ac_binary, Some(signer))
}

/// Pushes a fake agent-control binary package to the OCI registry and signs it.
fn push_signed_fake_ac_package(signer: &OCISigner) -> String {
    push_ac_package(build_fake_ac_binary, Some(signer))
}

/// Pushes a fake agent-control binary package to the OCI registry without signing it.
fn push_unsigned_fake_ac_package() -> String {
    push_ac_package(build_fake_ac_binary, None)
}

fn push_ac_package(build: fn() -> (TempDir, PathBuf), signer: Option<&OCISigner>) -> String {
    let dir = tempdir().unwrap();
    let (_binary_dir, binary_path) = build();

    #[cfg(target_family = "unix")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.tar.gz");
        TestDataHelper::compress_tar_gz_executable(&binary_path, &path);
        (path, PackageMediaType::TarGz)
    };

    #[cfg(target_family = "windows")]
    let (path, media_type) = {
        let path = dir.path().join("ac_package.zip");
        TestDataHelper::compress_zip_file(&binary_path, &path);
        (path, PackageMediaType::Zip)
    };

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&path, media_type);
    if let Some(signer) = signer {
        signer.sign_artifact(&reference);
    }
    reference.tag().unwrap().to_string()
}
