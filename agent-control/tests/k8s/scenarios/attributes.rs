use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, convert_to_vec_key_value,
};
use crate::common::opamp::ConfigResponse;
use crate::common::retry::retry;
use crate::common::{opamp::FakeServer, runtime::block_on};
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_PATH, wait_until_agent_control_with_opamp_is_started,
};
use crate::k8s::tools::{
    agent_control::start_agent_control_with_testdata_config, instance_id, k8s_env::K8sEnv,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_VERSION, CLUSTER_NAME_ATTRIBUTE_KEY, FLEET_ID_ATTRIBUTE_KEY,
    HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_CHART_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION,
    PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use nix::unistd::gethostname;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_test_attributes_from_existing_agent_type() {
    let test_name = "k8s_opamp_attributes_existing_agent_type";

    // setup the fake-opamp-server, with empty configuration for agents in local config local config should be used.
    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let expected_chart_version = "1.2.3-beta".to_string(); // Set in <test_name>/local-data-agent-control.template
    let instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::new_agent_control_id(),
    );
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

    let ac_expected_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OPAMP_SERVICE_NAMESPACE,
            Value::StringValue("newrelic".to_string()),
        ),
        (
            OPAMP_SERVICE_NAME,
            Value::StringValue("com.newrelic.agent_control".to_string()),
        ),
        (
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
            Value::StringValue(AGENT_CONTROL_VERSION.to_string()),
        ),
        (
            OPAMP_CHART_VERSION_ATTRIBUTE_KEY,
            Value::StringValue(expected_chart_version.to_string()),
        ),
    ]));

    let ac_expected_non_identifying_attributes = convert_to_vec_key_value(Vec::from([
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
            ac_expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &server,
            &instance_id,
            ac_expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    });

    let sub_agent_expected_identifying_attributes = convert_to_vec_key_value(Vec::from([
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

    let sub_agent_expected_non_identifying_attributes = convert_to_vec_key_value(Vec::from([
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
        let instance_id_sub_agent = instance_id::get_instance_id(
            k8s.client.clone(),
            &namespace,
            &AgentID::new("hello-world").unwrap(),
        );
        check_latest_identifying_attributes_match_expected(
            &server,
            &instance_id_sub_agent,
            sub_agent_expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &server,
            &instance_id_sub_agent,
            sub_agent_expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    })
}
