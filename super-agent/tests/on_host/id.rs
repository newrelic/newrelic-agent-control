use super::tools::config::create_file;
use crate::common::retry::retry;
use crate::common::super_agent::start_super_agent_with_custom_config;
use assert_cmd::Command;
use httpmock::Method::GET;
use httpmock::MockServer;
use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use newrelic_super_agent::super_agent::defaults::{
    DYNAMIC_AGENT_TYPE_FILENAME, SUPER_AGENT_CONFIG_FILE,
};
use newrelic_super_agent::super_agent::run::BasePaths;
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::system::detector::SystemDetector;
use std::time::Duration;
use tempfile::tempdir;

const UNRESPOSIVE_METADATA_ENDPOINT: &str = "http://localhost:9999";

#[test]
#[cfg(target_family = "unix")]
fn test_aws_cloud_id() {
    use httpmock::Method::PUT;

    use crate::on_host::consts::AWS_VM_RESPONSE;

    let metadata_path = "/latest/meta-data/instance-id";
    let token_path = "/token";
    let fake_token = "fake_token";
    let instance_id = "i-123456787d725bbe7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AWS_VM_RESPONSE);
    });
    let token_mock = fake_metadata_server.mock(|when, then| {
        when.method(PUT).path(token_path);
        then.status(200).body(fake_token);
    });

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            fake_metadata_server.url(metadata_path),
            fake_metadata_server.url(token_path),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
    token_mock.assert_calls(1);
}
#[test]
#[cfg(target_family = "unix")]
fn test_azure_cloud_id() {
    use crate::on_host::consts::AZURE_VM_RESPONSE;

    let metadata_path = "/metadata/instance";
    let instance_id = "02aab8a4-74ef-476e-8182-f6d2ba4166a7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AZURE_VM_RESPONSE);
    });

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            fake_metadata_server.url(metadata_path),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}

#[test]
#[cfg(target_family = "unix")]
fn test_gcp_cloud_id() {
    use crate::on_host::consts::GCP_VM_RESPONSE;

    let metadata_path = "/metadata/instance";
    let instance_id = "6331980990053453154";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(GCP_VM_RESPONSE);
    });

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            fake_metadata_server.url(metadata_path),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}

/// tests that nr-sa:host_id and nr-sub:agent_id are correctly replaced in the agent type.
#[cfg(unix)]
#[test]
fn test_sub_sa_vars() {
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_file(
        r#"
namespace: test
name: test
version: 0.0.0
variables: {}
deployment:
  on_host:
    executable:
      path: "sh"
      args: >-
        tests/on_host/data/trap_term_sleep_60.sh
        --host_id=${nr-sa:host_id}
        --agent_id=${nr-sub:agent_id}
    "#
        .to_string(),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );
    let sa_config_path = local_dir.path().join(SUPER_AGENT_CONFIG_FILE);
    create_file(
        r#"
host_id: fixed-host-id
agents:
  test-agent:
    agent_type: "test/test:0.0.0"
        "#
        .to_string(),
        sa_config_path.clone(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _super_agent = start_super_agent_with_custom_config(base_paths);

    retry(30, Duration::from_secs(1), || {
        // Check that the process is running with this exact command
        Command::new("pgrep")
            .arg("-f")
            .arg("sh tests/on_host/data/trap_term_sleep_60.sh --host_id=fixed-host-id --agent_id=test-agent")
            .assert().try_success()?;
        Ok(())
    });
}
