use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::http_port::{available_port, status_server_url};
use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_PATH, DUMMY_PRIVATE_KEY, DYNAMIC_AGENT_TYPE_FILENAME, K8S_KEY_SECRET,
    K8S_PRIVATE_KEY_SECRET, TEST_CLUSTER_NAME, create_config_map,
};
use crate::k8s::tools::config::K8sAgentControlConfigBuilder;
use crate::k8s::tools::k8s_api::create_values_secret;
use crate::k8s::tools::k8s_env::K8sEnv;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE, AGENT_CONTROL_VERSION,
    CLUSTER_NAME_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
};
use newrelic_agent_control::environment::Environment;
use serde_json::json;
use std::time::Duration;

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

    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let dirs = TempBasePaths::default();

    // Copy the k8s custom agent type into the dynamic agent types directory
    let agent_type_file_path = dirs.local_dir().join(DYNAMIC_AGENT_TYPE_FILENAME);
    std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
    std::fs::copy(CUSTOM_AGENT_TYPE_PATH, &agent_type_file_path).unwrap();

    let agents = format!(
        r#"
  {AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
"#
    );

    let status_server_port = available_port();
    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .with_status_server(status_server_port)
        .write(k8s.client.clone(), &dirs.local_dir());

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

    let _ac = start_agent_control_with_custom_config(dirs.base_paths(), Environment::K8s);

    let expected_fleet = json!({
        "enabled": true,
        "endpoint": opamp_server.endpoint(),
        "reachable": true,
    });

    retry(30, Duration::from_secs(1), || {
        let body: serde_json::Value = reqwest::blocking::get(status_server_url(status_server_port))
            .map_err(|e| format!("request failed: {e}"))?
            .json()
            .map_err(|e| format!("json parse failed: {e}"))?;

        if body["fleet"] != expected_fleet {
            return Err(format!("expected fleet={expected_fleet}, got: {}", body["fleet"]).into());
        }

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
                format!("non-identifying/{CLUSTER_NAME_ATTRIBUTE_KEY}"),
                json!(TEST_CLUSTER_NAME),
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

        let sub_attrs = &body["agents"][AGENT_ID]["attributes"];
        if !sub_attrs.is_object() {
            return Err(
                format!("agents.{AGENT_ID}.attributes not present or not an object").into(),
            );
        }
        let expected_sub_attrs = vec![
            (
                format!("identifying/{OPAMP_SERVICE_VERSION}"),
                json!("0.0.1"),
            ),
            (
                format!("non-identifying/{CLUSTER_NAME_ATTRIBUTE_KEY}"),
                json!(TEST_CLUSTER_NAME),
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

        Ok(())
    });
}
