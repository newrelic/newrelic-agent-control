use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::config::create_agent_control_config_with_status_server;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_VERSION, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use serde_json::json;
use std::net::TcpListener;
use std::time::Duration;
use tempfile::tempdir;

/// The /status endpoint should return the expected response shape:
/// - fleet fields reflect the configured OpAMP connection
/// - agents is empty when no sub-agents are configured
/// - agent_control.attributes contains the agent description derived from OpAMP start settings
#[test]
fn test_http_status_endpoint_response() {
    let opamp_server = FakeServer::start_new();
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let status_server_port = available_port();
    create_agent_control_config_with_status_server(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.path().to_path_buf(),
        status_server_port,
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _ac = start_agent_control_with_custom_config(base_paths, AGENT_CONTROL_MODE_ON_HOST);

    retry(30, Duration::from_secs(1), || {
        let body: serde_json::Value =
            reqwest::blocking::get(format!("http://127.0.0.1:{status_server_port}/status"))
                .map_err(|e| format!("request failed: {e}"))?
                .json()
                .map_err(|e| format!("json parse failed: {e}"))?;

        // fleet fields are set from config at server init, not event-driven
        if body["fleet"]["enabled"] != json!(true) {
            return Err(format!("expected fleet.enabled=true, got: {}", body["fleet"]).into());
        }

        // no sub-agents configured
        if body["agents"] != json!({}) {
            return Err(format!("expected empty agents, got: {}", body["agents"]).into());
        }

        // agent_control.attributes is populated
        let attrs = body["agent_control"]["attributes"]
            .as_object()
            .ok_or("agent_control.attributes not present or not an object")?;

        let got_version = attrs
            .get(OPAMP_AGENT_VERSION_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{OPAMP_AGENT_VERSION_ATTRIBUTE_KEY} missing"))?;
        if got_version != AGENT_CONTROL_VERSION {
            return Err(format!(
                "expected {OPAMP_AGENT_VERSION_ATTRIBUTE_KEY}={AGENT_CONTROL_VERSION}, got {got_version}"
            )
            .into());
        }

        let got_host_id = attrs
            .get(HOST_ID_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{HOST_ID_ATTRIBUTE_KEY} missing"))?;
        if got_host_id != "integration-test" {
            return Err(format!(
                "expected {HOST_ID_ATTRIBUTE_KEY}=integration-test, got {got_host_id}"
            )
            .into());
        }

        if attrs
            .get(HOST_NAME_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            return Err(format!("{HOST_NAME_ATTRIBUTE_KEY} is missing or empty").into());
        }

        Ok(())
    });
}

fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind ephemeral port")
        .local_addr()
        .unwrap()
        .port()
}
