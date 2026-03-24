use std::path::Path;

use assert_matches::assert_matches;
use newrelic_agent_control::agent_control::version_updater::on_host::{
    ProcessVerifyExecutor, VerifyError, VerifyExecutor,
};
use tempfile::tempdir;

use crate::{common::opamp::FakeServer, on_host::tools::config::create_agent_control_config};

/// Returns the path to the newrelic-agent-control binary under test
fn binary_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_newrelic-agent-control"))
}

#[test]
fn test_verify_executor() {
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let opamp_server = FakeServer::start_new();
    let agents = "{}".to_string();

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
    );

    let result = ProcessVerifyExecutor::default().execute(
        binary_path(),
        &[
            "--local-dir",
            local_dir.path().to_str().unwrap(),
            "--remote-dir",
            remote_dir.path().to_str().unwrap(),
            "verify",
        ],
    );

    assert!(
        result.is_ok(),
        "Expected verify to succeed with working OpAMP server, got error: {:?}",
        result.err()
    );
}

#[test]
fn test_verify_executor_read_config_error() {
    let folder_name = "folder-that-does-not-exist";

    let result = ProcessVerifyExecutor::default()
        .execute(binary_path(), &["--local-dir", folder_name, "verify"]);

    assert_matches!(result, Err(VerifyError::VerificationFailed { message, .. }) if message.contains(&format!("could not read Agent Control config from {}", folder_name)));
}

#[test]
fn test_verify_executor_opamp_connectivity_failure() {
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let unreachable_opamp_endpoint = "http://localhost:19999".to_string();
    let unreachable_jwks_endpoint = "http://localhost:19999/jwks".to_string();
    let agents = "{}".to_string();

    create_agent_control_config(
        unreachable_opamp_endpoint,
        unreachable_jwks_endpoint,
        agents,
        local_dir.path().to_path_buf(),
    );

    let result = ProcessVerifyExecutor::default().execute(
        binary_path(),
        &[
            "--local-dir",
            local_dir.path().to_str().unwrap(),
            "--remote-dir",
            remote_dir.path().to_str().unwrap(),
            "verify",
        ],
    );

    assert_matches!(
        result,
        Err(VerifyError::VerificationFailed { message, .. }) if message.contains("OpAMP connectivity check failed")
    );
}
