use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::http_port::{available_port, status_server_url};
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_PATH, DUMMY_PRIVATE_KEY, DYNAMIC_AGENT_TYPE_FILENAME, K8S_KEY_SECRET,
    K8S_PRIVATE_KEY_SECRET, TEST_CLUSTER_NAME, create_config_map,
    create_k8s_agent_control_config_with_status_server,
};
use crate::k8s::tools::k8s_api::create_values_secret;
use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_VERSION, CLUSTER_NAME_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_SERVICE_VERSION,
};
use newrelic_agent_control::agent_control::run::{BasePaths, Environment};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

/// The /status endpoint should return the expected response shape in k8s mode:
/// - fleet fields reflect the configured OpAMP connection
/// - agents contains the configured sub-agent with its attributes
/// - agent_control.attributes contains the agent description derived from OpAMP start settings,
///   including k8s-specific attributes like cluster.name
#[test]
#[ignore = "needs a k8s cluster"]
fn test_k8s_http_status_endpoint_response() {
    const AGENT_ID: &str = "hello-world";
    const AGENT_TYPE: &str = "newrelic/com.newrelic.custom_agent:0.0.1";

    let opamp_server = FakeServer::start_new();
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // Copy the k8s custom agent type into the dynamic agent types directory
    let agent_type_file_path = tmp_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME);
    std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
    std::fs::copy(CUSTOM_AGENT_TYPE_PATH, &agent_type_file_path).unwrap();

    let agents = format!(
        r#"
  {AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
"#
    );

    let status_server_port = available_port();
    create_k8s_agent_control_config_with_status_server(
        k8s.client.clone(),
        &namespace,
        &opamp_server.endpoint(),
        &opamp_server.jwks_endpoint(),
        status_server_port,
        tmp_dir.path(),
        &agents,
    );

    // Sub-agent values ConfigMap (empty chart_values)
    block_on(create_config_map(
        k8s.client.clone(),
        &namespace,
        AGENT_ID,
        "chart_values:\n".to_string(),
    ));

    create_values_secret(
        k8s.client.clone(),
        &namespace,
        K8S_PRIVATE_KEY_SECRET,
        K8S_KEY_SECRET,
        DUMMY_PRIVATE_KEY.to_string(),
    );

    let _ac = start_agent_control_with_custom_config(
        BasePaths {
            local_dir: tmp_dir.path().to_path_buf(),
            remote_dir: tmp_dir.path().join("remote"),
            log_dir: tmp_dir.path().join("log"),
        },
        Environment::K8s,
    );

    retry(30, Duration::from_secs(1), || {
        let body: serde_json::Value = reqwest::blocking::get(status_server_url(status_server_port))
            .map_err(|e| format!("request failed: {e}"))?
            .json()
            .map_err(|e| format!("json parse failed: {e}"))?;

        // fleet fields are set from config at server init, not event-driven
        if body["fleet"]["enabled"] != json!(true) {
            return Err(format!("expected fleet.enabled=true, got: {}", body["fleet"]).into());
        }

        // agent_control.attributes is populated by the AgentDescriptionUpdated event
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

        let got_cluster = ac_attrs
            .get(CLUSTER_NAME_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!("{CLUSTER_NAME_ATTRIBUTE_KEY} missing"))?;
        if got_cluster != TEST_CLUSTER_NAME {
            return Err(format!(
                "expected {CLUSTER_NAME_ATTRIBUTE_KEY}={TEST_CLUSTER_NAME}, got {got_cluster}"
            )
            .into());
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
        if got_service_version != "0.0.1" {
            return Err(format!(
                "expected sub-agent {OPAMP_SERVICE_VERSION}=0.0.1, got {got_service_version}"
            )
            .into());
        }

        let got_sub_cluster = sub_attrs
            .get(CLUSTER_NAME_ATTRIBUTE_KEY)
            .and_then(|v| v.as_str())
            .ok_or(format!(
                "{CLUSTER_NAME_ATTRIBUTE_KEY} missing from sub-agent"
            ))?;
        if got_sub_cluster != TEST_CLUSTER_NAME {
            return Err(format!(
                "expected sub-agent {CLUSTER_NAME_ATTRIBUTE_KEY}={TEST_CLUSTER_NAME}, got {got_sub_cluster}"
            )
            .into());
        }

        Ok(())
    });
}
