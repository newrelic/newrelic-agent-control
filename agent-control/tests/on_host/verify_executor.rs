use std::path::Path;

use assert_matches::assert_matches;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::EXECUTION_MODE_ATTRIBUTE_KEY;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::version_updater::on_host::verify::{
    ProcessVerifyExecutor, VerifyError, VerifyExecutor,
};
use opamp_client::opamp::proto::any_value::Value;
use tempfile::tempdir;

use crate::{
    common::runtime::tokio_runtime, on_host::tools::config::create_agent_control_config,
    on_host::tools::instance_id::get_instance_id,
};
use fake_opamp_server::FakeServer;

/// Returns the path to the newrelic-agent-control binary under test
fn binary_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_newrelic-agent-control"))
}

#[test]
fn test_verify_executor() {
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let agents = "{}".to_string();

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

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

    // Verify that execution.mode attribute was sent to OpAMP server
    let agent_control_instance_id = get_instance_id(&AgentID::AgentControl, base_paths);
    let attributes = opamp_server
        .get_attributes(agent_control_instance_id.clone())
        .expect("Agent Control attributes not found in OpAMP server");

    let execution_mode_attr = attributes
        .non_identifying_attributes
        .iter()
        .find(|kv| kv.key == EXECUTION_MODE_ATTRIBUTE_KEY);

    assert!(
        execution_mode_attr.is_some(),
        "execution.mode attribute not found in non-identifying attributes"
    );

    let attr_value = execution_mode_attr
        .unwrap()
        .value
        .as_ref()
        .and_then(|v| v.value.as_ref());

    match attr_value {
        Some(Value::StringValue(val)) => {
            assert_eq!(
                val, "dry-run",
                "execution.mode attribute should have value 'dry-run', found '{}'",
                val
            );
        }
        _ => panic!("execution.mode attribute should be a string value with 'dry-run'"),
    }
}

#[test]
fn test_verify_executor_read_config_error() {
    let folder_name = "folder-that-does-not-exist";

    let result = ProcessVerifyExecutor::default()
        .execute(binary_path(), &["--local-dir", folder_name, "verify"]);

    assert_matches!(result, Err(VerifyError::VerificationFailed(msg)) if msg.contains(&format!("could not read Agent Control config from {}", folder_name)));
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
        Err(VerifyError::VerificationFailed(msg)) if msg.contains("OpAMP connectivity check failed")
    );
}
