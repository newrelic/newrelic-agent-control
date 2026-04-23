//! Integration tests for k8s without Flux
//! These tests use a simplified environment with just agent-control binary and image.

use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::start_agent_control_with_testdata_config;
use crate::k8s::tools::k8s_api::check_config_map_exist;
use crate::k8s::tools::k8s_env::K8sEnv;
use crate::k8s::tools::{agent_control, instance_id};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use std::time::Duration;
use tempfile::tempdir;

const CONFIG_AGENT_TYPE_PATH: &str = "tests/k8s/data/config_map_type.yml";

/// This test verifies that the config_map_type agent type creates
/// a ConfigMap when configured through OpAMP.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_config_map_type_creates_configmap() {
    let test_name = "k8s_config_map_type_creates_configmap";

    let mut server = FakeServer::start_new();

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let _ac = start_agent_control_with_testdata_config(
        test_name,
        CONFIG_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        &namespace,
        Some(&server.endpoint()),
        Some(&server.jwks_endpoint()),
        Vec::new(),
        tmp_dir.path(),
    );

    agent_control::wait_until_agent_control_with_opamp_is_started(
        k8s.client.clone(),
        namespace.as_str(),
    );

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    server.set_config_response(
        instance_id.clone(),
        r#"
agents:
  test-config-map:
    agent_type: "newrelic/com.newrelic.test_config_map:0.1.0"
    "#,
    );

    println!("Waiting for fleet-data ConfigMap to be created...");
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_exist(
            k8s.client.clone(),
            "fleet-data-test-config-map",
            &namespace,
        ))
    });
    println!("fleet-data ConfigMap exists!");

    let subagent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::try_from("test-config-map").unwrap(),
    );
    server.set_config_response(
        subagent_instance_id,
        r#"
chart_values:
  cm_content:
    some_key: "some_value"
    "#,
    );

    println!("Waiting for test-config-map ConfigMap...");

    let expected_configmap_name = "test-config-map";
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_exist(
            k8s.client.clone(),
            expected_configmap_name,
            &namespace,
        ))
    });
    println!("ConfigMap {} exists!", expected_configmap_name);

    let api: Api<ConfigMap> = Api::namespaced(k8s.client.clone(), &namespace);
    // Verify the ConfigMap content
    let cm = block_on(api.get(expected_configmap_name)).expect("ConfigMap should exist");

    // Check that the ConfigMap has the expected data keys
    let data = cm.data.expect("ConfigMap should have data");
    let deployment_config = data
        .get("deployment-config.yaml")
        .expect("deployment-config.yaml should exist");
    assert!(
        deployment_config.contains("some_key"),
        "deployment-config.yaml should contain the generated configuration"
    );
    assert!(
        deployment_config.contains("some_value"),
        "deployment-config.yaml should contain the configured values"
    );

    // Test removal: remove the agent and verify the ConfigMap is deleted
    server.set_config_response(
        instance_id.clone(),
        r#"
agents: {}
    "#,
    );

    // Verify that the ConfigMap is removed (garbage collected)
    retry(120, Duration::from_secs(1), || {
        if block_on(api.get(expected_configmap_name)).is_ok() {
            return Err("ConfigMap still exists".into());
        }
        Ok(())
    });
}
