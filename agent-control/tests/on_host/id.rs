use super::tools::config::{create_file, create_local_config};
use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::process_finder::find_processes_by_pattern;
use crate::common::retry::retry;
use crate::on_host::consts::AWS_VM_RESPONSE;
use crate::on_host::consts::AZURE_VM_RESPONSE;
use crate::on_host::consts::GCP_VM_RESPONSE;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME;
use httpmock::Method::GET;
use httpmock::Method::PUT;
use httpmock::MockServer;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::Environment;
use newrelic_agent_control::http::client::HttpClient;
use newrelic_agent_control::http::config::{HttpConfig, ProxyConfig};
use newrelic_agent_control::on_host::file_store::build_config_name;
use newrelic_agent_control::opamp::instance_id::on_host::identifiers::IdentifiersProvider;
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use resource_detection::system::detector::SystemDetector;
use std::time::Duration;
use tempfile::tempdir;

const UNRESPONSIVE_METADATA_ENDPOINT: &str = "http://localhost:9999";

#[test]
fn test_aws_cloud_id() {
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

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        fake_metadata_server.url(metadata_path),
        fake_metadata_server.url(token_path),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
    token_mock.assert_calls(1);
}

#[test]
fn test_azure_cloud_id() {
    let metadata_path = "/metadata/instance";
    let instance_id = "02aab8a4-74ef-476e-8182-f6d2ba4166a7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AZURE_VM_RESPONSE);
    });

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        fake_metadata_server.url(metadata_path),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}

#[test]
fn test_gcp_cloud_id() {
    let metadata_path = "/metadata/instance";
    let instance_id = "6331980990053453154";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(GCP_VM_RESPONSE);
    });

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        fake_metadata_server.url(metadata_path),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}

/// tests that nr-ac:host_id and nr-sub:agent_id are correctly replaced in the agent type.
#[test]
fn test_sub_sa_vars() {
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    #[cfg(target_family = "unix")]
    create_file(
        r#"
namespace: test
name: test
version: 0.0.0
variables: {}
deployment:
  on_host:
    executables:
      - id: trap-term-sleep
        path: "sh"
        args: >-
          tests/on_host/data/trap_term_sleep_60.sh
          --host_id=${nr-ac:host_id}
          --agent_id=${nr-sub:agent_id}
    "#
        .to_string(),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    #[cfg(target_family = "windows")]
    create_file(
        r#"
namespace: test
name: test
version: 0.0.0
variables: {}
deployment:
  on_host:
    executables:
      - id: trap-term-sleep
        path: "powershell.exe"
        args: >-
          -NoProfile -ExecutionPolicy Bypass
          -File tests\\on_host\\data\\trap_term_sleep_60.ps1
          --host_id=${nr-ac:host_id}
          --agent_id=${nr-sub:agent_id}
    "#
        .to_string(),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    let sa_config_path = local_dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID)
        .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));
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
    create_local_config(
        "test-agent".into(),
        NO_CONFIG.to_string(),
        local_dir.path().into(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control = start_agent_control_with_custom_config(base_paths, Environment::OnHost);

    retry(30, Duration::from_secs(1), || {
        // Check that the process is running with this exact command
        if find_processes_by_pattern("trap_term_sleep_60").is_empty() {
            Err("process not found".into())
        } else {
            Ok(())
        }
    });
}
