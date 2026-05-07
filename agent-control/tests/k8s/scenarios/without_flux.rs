//! Integration tests for k8s without Flux
//! These tests use a simplified environment with just agent-control binary and image.

use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::start_agent_control_with_testdata_config;
use crate::k8s::tools::k8s_api::{check_config_map_exist, check_config_map_has_annotation};
use crate::k8s::tools::k8s_env::K8sEnv;
use crate::k8s::tools::{agent_control, instance_id};
use fake_opamp_server::FakeServer;
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

    let mut server = FakeServer::start(tokio_runtime().handle());

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

    // Wait for the annotation to be written
    println!("Waiting for agent-type-id annotation on fleet-data ConfigMap...");
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_has_annotation(
            k8s.client.clone(),
            expected_configmap_name,
            &namespace,
            "newrelic.io/agent-type-id",
        ))
    });
    println!("Annotation present.");

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

/// This test verifies that GC handles the fleet-data ConfigMap correctly on AC restart.
///
/// Fleet-data ConfigMaps are Agent Control internal resources and carry the
/// `newrelic.io/owned-by: agent-control` annotation. They are handled by
/// `garbage_collect_agent_control_resources`, which lists ConfigMaps by label and
/// deletes only those with the `owned-by: agent-control` annotation.
///
/// Sub-agent dynamic objects (e.g. supervisor-created resources) carry the
/// `newrelic.io/owned-by: sub-agent` annotation and are handled by
/// `garbage_collect_sub_agent_resources`.
///
/// The test:
/// 1. Starts AC and deploys the config-map-type agent via OpAMP remote config.
/// 2. Sends a remote config to the sub-agent, which causes AC to store the remote config
///    and write the `owned-by: agent-control` and `agent-type-id` annotations onto the
///    fleet-data ConfigMap.
/// 3. Waits for the annotation to be present, then stops and restarts AC.
/// 4. On restart, `retain` finds the annotated fleet-data ConfigMap, recognises it as an
///    Agent Control internal resource, and correctly retains it.
/// 5. The test passes only if AC starts successfully.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_config_map_type_gc_does_not_fail_on_restart() {
    let test_name = "k8s_config_map_type_gc_does_not_fail_on_restart";

    let mut server = FakeServer::start(tokio_runtime().handle());

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

    // Deploy the config-map-type agent via the fleet-level config.
    server.set_config_response(
        instance_id.clone(),
        r#"
agents:
  test-config-map:
    agent_type: "newrelic/com.newrelic.test_config_map:0.1.0"
    "#,
    );

    // Wait for the fleet-data ConfigMap to be created (instance-ID written by the storer).
    let fleet_data_cm_name = "fleet-data-test-config-map";
    println!("Waiting for fleet-data ConfigMap to be created...");
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_exist(
            k8s.client.clone(),
            fleet_data_cm_name,
            &namespace,
        ))
    });

    // Send a remote config to the sub-agent. This causes AC to call `store_remote` for the
    // sub-agent, which writes the agent-type-id annotation onto the fleet-data ConfigMap.
    let subagent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::try_from("test-config-map").unwrap(),
    );
    server.set_config_response(
        subagent_instance_id,
        r#"
chart_values:
  cm_content: {}
    "#,
    );

    // Wait for the annotation to be written
    println!("Waiting for agent-type-id annotation on fleet-data ConfigMap...");
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_has_annotation(
            k8s.client.clone(),
            fleet_data_cm_name,
            &namespace,
            "newrelic.io/agent-type-id",
        ))
    });
    println!("Annotation present — stopping AC to simulate a restart.");

    // Stop AC while the agent is still active. The annotated fleet-data ConfigMap persists.
    drop(_ac);

    // Restart AC with the same configuration. On startup, `retain` is called with the
    // active agent ({test-config-map: newrelic/com.newrelic.test_config_map:0.1.0}) and
    // cr_type_meta includes ConfigMap. GC finds the fleet-data ConfigMap, reads the
    // agent-type-id annotation, and correctly retains it.
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

    // If GC correctly handles the annotated fleet-data ConfigMap on restart, AC starts
    // successfully. If not, this will time out because AC crashes before creating
    // fleet-data-agent-control.
    agent_control::wait_until_agent_control_with_opamp_is_started(
        k8s.client.clone(),
        namespace.as_str(),
    );
}
