use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::config::{create_agent_control_config, create_file};
use crate::on_host::tools::instance_id::get_instance_id;
use httpmock::Method::GET;
use httpmock::MockServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::DYNAMIC_AGENT_TYPE_FILENAME;
use newrelic_agent_control::agent_control::run::BasePaths;
use std::time::Duration;
use tempfile::tempdir;
/// Given a agent-control with a sub-agent without supervised executables, it should be able to
/// read the health status from the file and send it to the opamp server.
#[cfg(unix)]
#[test]
fn test_file_health_without_supervisor() {
    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let health_file_path = local_dir.path().join("health_file.yaml");

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables: {{}}
deployment:
  on_host:
    health:
      interval: 1s
      timeout: 1s
      file:
        path: {}
"#,
            health_file_path.to_str().unwrap()
        ),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    let agents = r#"
  test-agent:
    agent_type: "test/test:0.0.0"
"#;

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control = start_agent_control_with_custom_config(base_paths.clone());

    let agent_control_instance_id =
        get_instance_id(&AgentID::new("test-agent").unwrap(), base_paths);

    create_file(
        r#"
healthy: true
status: "healthy-message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001  
    "#
        .to_string(),
        health_file_path.clone(),
    );

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) = opamp_server.get_health_status(&agent_control_instance_id) {
            if health_status.healthy
                && health_status.status == "healthy-message"
                && health_status.start_time_unix_nano == 1725444000
                && health_status.status_time_unix_nano == 1725444001
            {
                return Ok(());
            }
        }
        Err("Healthy status not found".into())
    });

    create_file(
        r#"
healthy: false
status: "unhealthy-message"
last_error: "error-message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444002 
"#
        .to_string(),
        health_file_path.clone(),
    );

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) = opamp_server.get_health_status(&agent_control_instance_id) {
            if !health_status.healthy
                && health_status.status == "unhealthy-message"
                && health_status.last_error == "error-message"
                && health_status.start_time_unix_nano == 1725444000
                && health_status.status_time_unix_nano == 1725444002
            {
                return Ok(());
            }
        }
        Err("Unhealthy status not found".into())
    });
}

/// Given a agent-control with a sub-agent without supervised executables, it should be able to
/// read the health status from http endpoint and send it to the opamp server.
#[cfg(unix)]
#[test]
fn test_http_health_without_supervisor() {
    let opamp_server = FakeServer::start_new();

    let health_server = MockServer::start();

    let mut healthy_mock = health_server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).body(r#"healthy-message"#);
    });

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables: {{}}
deployment:
  on_host:
    health:
      interval: 1s
      timeout: 1s
      http:
        path: /health
        port: {}
"#,
            health_server.port()
        ),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    let agents = r#"
  test-agent:
    agent_type: "test/test:0.0.0"
"#;

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let base_paths = base_paths.clone();

    let _agent_control = start_agent_control_with_custom_config(base_paths.clone());

    let agent_control_instance_id =
        get_instance_id(&AgentID::new("test-agent").unwrap(), base_paths);

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) = opamp_server.get_health_status(&agent_control_instance_id) {
            if health_status.healthy && health_status.status == "healthy-message" {
                return Ok(());
            }
        }
        Err("Healthy status not found".into())
    });

    healthy_mock.delete();

    health_server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(500).body(r#"unhealthy-message"#);
    });

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) = opamp_server.get_health_status(&agent_control_instance_id) {
            if !health_status.healthy && health_status.status == "unhealthy-message" {
                return Ok(());
            }
        }
        Err("Unhealthy status not found".into())
    });
}
