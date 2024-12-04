use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, convert_to_vec_key_value,
};
use crate::common::opamp::ConfigResponse;
use crate::common::retry::retry;
use crate::common::{opamp::FakeServer, runtime::block_on};
use crate::k8s::tools::super_agent::{
    wait_until_super_agent_with_opamp_is_started, CUSTOM_AGENT_TYPE_PATH,
};
use crate::k8s::tools::{
    instance_id, k8s_env::K8sEnv, super_agent::start_super_agent_with_testdata_config,
};
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::{
    CLUSTER_NAME_ATTRIBUTE_KEY, FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_CHART_VERSION_ATTRIBUTE_KEY, OPAMP_SERVICE_NAME,
    OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use nix::unistd::gethostname;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;

/// This scenario tests an Agent type which only create a CR when the CRD already exists.
/// The sub-agent is added from remote config and them we check if the agent description is what we expect.
#[test]
#[ignore = "needs a k8s cluster"]
#[serial]
fn test_attributes_from_existing_agent_type() {
    let test_name = "k8s_opamp_add_sub_agent";

    // setup the fake-opamp-server, with empty configuration for agents in local config local config should be used.
    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        Some(&server.endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let instance_id = instance_id::get_instance_id(&namespace, &AgentID::new_super_agent_id());
    server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(
            r#"
agents:
  hello-world:
    agent_type: "newrelic/com.newrelic.custom_agent:0.0.1"
            "#,
        ),
    );

    let expected_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OPAMP_SERVICE_NAMESPACE,
            Value::StringValue("newrelic".to_string()),
        ),
        (
            OPAMP_SERVICE_NAME,
            Value::StringValue("com.newrelic.super_agent".to_string()),
        ),
        (
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
            Value::StringValue("0.26.0".to_string()),
        ),
    ]));

    let expected_non_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            HOST_NAME_ATTRIBUTE_KEY,
            Value::StringValue(gethostname().unwrap_or_default().into_string().unwrap()),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY,
            Value::StringValue(String::default()),
        ),
        (
            CLUSTER_NAME_ATTRIBUTE_KEY,
            Value::StringValue("minikube".to_string()),
        ),
    ]));

    // Check attributes of Agent Control
    retry(60, Duration::from_secs(5), || {
        check_latest_identifying_attributes_match_expected(
            &server,
            &instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &server,
            &instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    });

    let expected_identifying_attributes_sub_agent = convert_to_vec_key_value(Vec::from([
        (
            OPAMP_SERVICE_NAMESPACE,
            Value::StringValue("newrelic".to_string()),
        ),
        (
            OPAMP_SERVICE_NAME,
            Value::StringValue("com.newrelic.custom_agent".to_string()),
        ),
        (
            OPAMP_SERVICE_VERSION,
            Value::StringValue("0.0.1".to_string()),
        ),
        (
            OPAMP_CHART_VERSION_ATTRIBUTE_KEY,
            Value::StringValue("0.1.0".to_string()),
        ),
    ]));

    let expected_non_identifying_attributes_sub_agent = convert_to_vec_key_value(Vec::from([
        (
            CLUSTER_NAME_ATTRIBUTE_KEY,
            Value::StringValue("minikube".to_string()),
        ),
        (
            PARENT_AGENT_ID_ATTRIBUTE_KEY,
            BytesValue(instance_id.clone().into()),
        ),
    ]));

    // Check attributes of sub agent
    retry(90, Duration::from_secs(5), || {
        let instance_id_sub_agent =
            instance_id::get_instance_id(&namespace, &AgentID::new("hello-world").unwrap());
        check_latest_identifying_attributes_match_expected(
            &server,
            &instance_id_sub_agent,
            expected_identifying_attributes_sub_agent.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &server,
            &instance_id_sub_agent,
            expected_non_identifying_attributes_sub_agent.clone(),
        )?;
        Ok(())
    })
}
