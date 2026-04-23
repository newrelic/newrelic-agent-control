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
    AGENT_CONTROL_ID, AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE, AGENT_CONTROL_VERSION,
    HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
    OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
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

    let expected_fleet = json!({
        "enabled": true,
        "endpoint": opamp_server.endpoint(),
        "reachable": true,
    });

    retry(30, Duration::from_secs(1), || {
        // Query the status server endpoint
        let body: serde_json::Value = reqwest::blocking::get(status_server_url(status_server_port))
            .map_err(|e| format!("request failed: {e}"))?
            .json()
            .map_err(|e| format!("json parse failed: {e}"))?;

        // Check fleet sections
        if body["fleet"] != expected_fleet {
            return Err(format!("expected fleet={expected_fleet}, got: {}", body["fleet"]).into());
        }

        // Check Agent Control attributes
        let ac_attrs = &body["agent_control"]["attributes"];
        if !ac_attrs.is_object() {
            return Err("agent_control.attributes not present or not an object".into());
        }
        let expected_ac_attrs = vec![
            (
                format!("identifying/{OPAMP_AGENT_VERSION_ATTRIBUTE_KEY}"),
                json!(AGENT_CONTROL_VERSION),
            ),
            (
                format!("non-identifying/{HOST_ID_ATTRIBUTE_KEY}"),
                json!(HOST_ID),
            ),
            (
                format!("identifying/{OPAMP_SERVICE_NAME}"),
                json!(AGENT_CONTROL_TYPE),
            ),
            (
                format!("identifying/{OPAMP_SERVICE_NAMESPACE}"),
                json!(AGENT_CONTROL_NAMESPACE),
            ),
            (
                format!("identifying/{OPAMP_SUPERVISOR_KEY}"),
                json!(AGENT_CONTROL_ID),
            ),
        ];
        for (key, expected) in &expected_ac_attrs {
            if ac_attrs[key.as_str()] != *expected {
                return Err(
                    format!("expected {key}={expected}, got: {}", ac_attrs[key.as_str()]).into(),
                );
            }
        }
        let ac_host_name_key = format!("non-identifying/{HOST_NAME_ATTRIBUTE_KEY}");
        if ac_attrs[ac_host_name_key.as_str()]
            .as_str()
            .unwrap_or("")
            .is_empty()
        {
            return Err(format!("{ac_host_name_key} is missing or empty").into());
        }

        // Check sub-agent attributes
        let sub_attrs = &body["agents"][AGENT_ID]["attributes"];
        if !sub_attrs.is_object() {
            return Err(
                format!("agents.{AGENT_ID}.attributes not present or not an object").into(),
            );
        }
        let expected_sub_attrs = vec![
            (
                format!("identifying/{OPAMP_SERVICE_VERSION}"),
                json!("0.1.0"),
            ),
            (
                format!("non-identifying/{OS_ATTRIBUTE_KEY}"),
                json!(OS_ATTRIBUTE_VALUE),
            ),
            (
                format!("identifying/{OPAMP_SERVICE_NAME}"),
                json!("com.newrelic.custom_agent"),
            ),
            (
                format!("identifying/{OPAMP_SERVICE_NAMESPACE}"),
                json!(AGENT_CONTROL_NAMESPACE),
            ),
            (
                format!("identifying/{OPAMP_SUPERVISOR_KEY}"),
                json!(AGENT_ID),
            ),
        ];
        for (key, expected) in &expected_sub_attrs {
            if sub_attrs[key.as_str()] != *expected {
                return Err(format!(
                    "expected sub {key}={expected}, got: {}",
                    sub_attrs[key.as_str()]
                )
                .into());
            }
        }
        let sub_host_name_key = format!("non-identifying/{HOST_NAME_ATTRIBUTE_KEY}");
        if sub_attrs[sub_host_name_key.as_str()]
            .as_str()
            .unwrap_or("")
            .is_empty()
        {
            return Err(format!("sub-agent {sub_host_name_key} is missing or empty").into());
        }

        Ok(())
    });
}
