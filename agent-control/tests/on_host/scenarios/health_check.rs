use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::tokio_runtime;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::{
    OnHostAgentControlConfigBuilder, create_file, create_local_config,
};
use crate::on_host::tools::custom_agent_type::OnHostCustomAgentTypeBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use httpmock::Method::GET;
use httpmock::MockServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;

/// Given a agent-control with a sub-agent without supervised executables, it should be able to
/// read the health status from the file and send it to the opamp server.
#[test]
fn test_file_health_without_supervisor() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();
    let sub_agent_id = AgentID::try_from("test-agent").unwrap();

    let health_file_path = dirs.local_dir().join("health_file.yaml");
    let health_config = format!(
        r#"
interval: 1s
initial_delay: 0s
timeout: 1s
checks:
  - kind: File
    path: '{}'
"#,
        health_file_path.to_str().unwrap()
    );

    let agent_type = OnHostCustomAgentTypeBuilder::empty()
        .with_health(Some(&health_config))
        .write(dirs.local_dir());

    create_local_config(
        sub_agent_id.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(sub_agent_id.to_string(), agent_type)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    create_file(
        r#"
healthy: true
status: "healthy-message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444200
    "#
        .to_string(),
        health_file_path.clone(),
    );

    retry(30, Duration::from_secs(1), || {
        // health_status.start_time_unix_nano and health_status.status_time_unix_nano are not going
        // to match the ones from the file because they will be the ones from the aggregated checker
        if let Some(health_status) =
            opamp_server.get_health_status(agent_control_instance_id.clone())
            && health_status.healthy
            && health_status.status == "healthy-message"
        {
            return Ok(());
        }
        Err("Healthy status not found".into())
    });

    create_file(
        r#"
healthy: false
status: "unhealthy-message"
last_error: "error-message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444500
"#
        .to_string(),
        health_file_path.clone(),
    );

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) =
            opamp_server.get_health_status(agent_control_instance_id.clone())
            && !health_status.healthy
            && health_status.status == "unhealthy-message"
            && health_status.last_error == "error-message"
            && health_status.start_time_unix_nano == 1725444000
            && health_status.status_time_unix_nano == 1725444500
        {
            return Ok(());
        }
        Err("Unhealthy status not found".into())
    });
}

/// Given a agent-control with a sub-agent without supervised executables, it should be able to
/// read the health status from http endpoint and send it to the opamp server.
#[test]
fn test_http_health_without_supervisor() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let health_server = MockServer::start();

    let mut healthy_mock = health_server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).body(r#"healthy-message"#);
    });

    let dirs = TempBasePaths::default();
    let sub_agent_id = AgentID::try_from("test-agent").unwrap();

    let health_config = format!(
        r#"
interval: 1s
initial_delay: 0s
timeout: 1s
checks:
  - kind: Http
    path: /health
    port: {}
"#,
        health_server.port(),
    );

    let agent_type = OnHostCustomAgentTypeBuilder::empty()
        .with_health(Some(&health_config))
        .write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(sub_agent_id.to_string(), agent_type)
        .write(dirs.local_dir());
    create_local_config(
        sub_agent_id.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) =
            opamp_server.get_health_status(agent_control_instance_id.clone())
            && health_status.healthy
            && health_status.status == "healthy-message"
        {
            return Ok(());
        }
        Err("Healthy status not found".into())
    });

    healthy_mock.delete();

    health_server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(500).body(r#"unhealthy-message"#);
    });

    retry(30, Duration::from_secs(1), || {
        if let Some(health_status) =
            opamp_server.get_health_status(agent_control_instance_id.clone())
            && !health_status.healthy
            && health_status.status == "unhealthy-message"
        {
            return Ok(());
        }
        Err("Unhealthy status not found".into())
    });
}

/// When an on-host sub-agent uses an agent type that does NOT define a `health:` block, no
/// `ComponentHealth` should be recorded for the sub-agent even if its initial supervisor fails to
/// start.
#[test]
fn test_no_health_in_agent_type_when_start_failure() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();
    let sub_agent_id = AgentID::try_from("test-agent-broken-fs").unwrap();

    let agent_type = OnHostCustomAgentTypeBuilder::empty()
        .with_health(None)
        .with_variables(
            r#"
fake_var:
  description: "Contents of the file"
  type: "string"
  required: true
"#,
        )
        .write(dirs.local_dir());

    create_local_config(
        sub_agent_id.to_string(),
        "some: broken config".to_string(),
        dirs.local_dir(),
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(sub_agent_id.to_string(), agent_type)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    opamp_server.set_config_response(sub_agent_instance_id.clone(), "some: broken config");

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )
    });

    // No health status is expected, despite the supervisor start failure.
    retry_never(10, Duration::from_secs(1), || {
        match opamp_server.get_health_status(sub_agent_instance_id.clone()) {
            None => Ok(()),
            Some(health) => Err(format!(
                "Expected no ComponentHealth for sub-agent without `health:` in agent type, got: {health:?}"
            )
            .into()),
        }
    });
}

/// When an on-host sub-agent uses an agent type that does define a `health:` block, health should be reported
/// if its initial supervisor fails to start.
#[test]
fn test_health_in_agent_type_when_start_failure() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();
    let sub_agent_id = AgentID::try_from("test-agent-broken-fs").unwrap();

    let agent_type = OnHostCustomAgentTypeBuilder::empty()
        .with_variables(
            r#"
fake_var:
  description: "Contents of the file"
  type: "string"
  required: true
"#,
        )
        .with_health(Some(
            r#"{"interval": "1s", "initial_delay": "2s", "checks": [{ "kind": "Process"}]}"#,
        ))
        .write(dirs.local_dir());

    create_local_config(
        sub_agent_id.to_string(),
        "some: broken config".to_string(),
        dirs.local_dir(),
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(sub_agent_id.to_string(), agent_type)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    opamp_server.set_config_response(sub_agent_instance_id.clone(), "some: broken config");

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;
        match opamp_server.get_health_status(sub_agent_instance_id.clone()) {
            Some(_) => Ok(()),
            None => Err("Expected ComponentHealth for sub-agent".into()),
        }
    });
}
