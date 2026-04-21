use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::http_port::{available_port, status_server_url};
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::{
    create_agent_control_config_with_status_server, create_local_config,
};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_VERSION, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_SERVICE_VERSION, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

/// The /status endpoint should return the expected response shape:
/// - fleet fields reflect the configured OpAMP connection
/// - agents contains the configured sub-agent with its attributes
/// - agent_control.attributes contains the agent description derived from OpAMP start settings
#[test]
fn test_http_status_endpoint_response() {
    const AGENT_ID: &str = "nr-sleep-agent";
    const HOST_ID: &str = "integration-test";

    let opamp_server = FakeServer::start_new();
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = CustomAgentType::default().build(local_dir.path().to_path_buf());

    let agents = format!(
        r#"
  {AGENT_ID}:
    agent_type: "{sleep_agent_type}"
"#
    );

    let status_server_port = available_port();
    create_agent_control_config_with_status_server(
        HOST_ID,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
        status_server_port,
    );

    create_local_config(
        AGENT_ID.to_string(),
        NO_CONFIG.to_string(),
        local_dir.path().into(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _ac = start_agent_control_with_custom_config(base_paths, AGENT_CONTROL_MODE_ON_HOST);

    retry(30, Duration::from_secs(1), || {
        let body: serde_json::Value = reqwest::blocking::get(status_server_url(status_server_port))
            .map_err(|e| format!("request failed: {e}"))?
            .json()
            .map_err(|e| format!("json parse failed: {e}"))?;

        // fleet fields are set from config at server init, not event-driven
        if body["fleet"]["enabled"] != json!(true) {
            return Err(format!("expected fleet.enabled=true, got: {}", body["fleet"]).into());
        }

        // agent_control.attributes is populated
        let ac_attrs = body["agent_control"]["attributes"]
            .as_object()
            .ok_or("agent_control.attributes not present or not an object")?;

        let got_version = ac_attrs
            .get(OPAMP_AGENT_VERSION_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{OPAMP_AGENT_VERSION_ATTRIBUTE_KEY} missing"))?;
        if got_version != AGENT_CONTROL_VERSION {
            return Err(format!(
                "expected {OPAMP_AGENT_VERSION_ATTRIBUTE_KEY}={AGENT_CONTROL_VERSION}, got {got_version}"
            )
            .into());
        }

        let got_host_id = ac_attrs
            .get(HOST_ID_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{HOST_ID_ATTRIBUTE_KEY} missing"))?;
        if got_host_id != HOST_ID {
            return Err(
                format!("expected {HOST_ID_ATTRIBUTE_KEY}={HOST_ID}, got {got_host_id}").into(),
            );
        }

        if ac_attrs
            .get(HOST_NAME_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            return Err(format!("{HOST_NAME_ATTRIBUTE_KEY} is missing or empty").into());
        }

        // sub-agent attributes are populated from AgentDescriptionSet
        let sub_attrs = body["agents"][AGENT_ID]["attributes"]
            .as_object()
            .ok_or(format!(
                "agents.{AGENT_ID}.attributes not present or not an object"
            ))?;

        let got_service_version = sub_attrs
            .get(OPAMP_SERVICE_VERSION)
            .and_then(|v| v.as_str())
            .ok_or(format!("{OPAMP_SERVICE_VERSION} missing from sub-agent"))?;
        if got_service_version != "0.1.0" {
            return Err(format!(
                "expected sub-agent {OPAMP_SERVICE_VERSION}=0.1.0, got {got_service_version}"
            )
            .into());
        }

        if sub_attrs
            .get(HOST_NAME_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            return Err(format!("sub-agent {HOST_NAME_ATTRIBUTE_KEY} is missing or empty").into());
        }

        let got_os = sub_attrs
            .get(OS_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{OS_ATTRIBUTE_KEY} missing from sub-agent"))?;
        if got_os != OS_ATTRIBUTE_VALUE {
            return Err(format!(
                "expected sub-agent {OS_ATTRIBUTE_KEY}={OS_ATTRIBUTE_VALUE}, got {got_os}"
            )
            .into());
        }

        Ok(())
    });
}
