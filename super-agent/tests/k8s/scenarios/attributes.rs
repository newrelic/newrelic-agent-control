use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected, get_expected_identifying_attributes,
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
use std::time::Duration;
use tempfile::tempdir;

const DEFAULT_VERSION: &str = "0.3.0";
#[cfg(unix)]
#[test]
fn test_attributes_from_non_existing_agent_type() {
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
    let instance_id_sub_agent =
        instance_id::get_instance_id(&namespace, &AgentID::new("hello-world").unwrap());

    println!("SUB AGENT ID: {:?}", instance_id_sub_agent);
    println!("SUPER AGENT ID: {:?}", instance_id);

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

    let expected_identifying_attributes = get_expected_identifying_attributes(
        "newrelic".to_string(),
        "com.newrelic.super_agent".to_string(),
        None,
        Some("0.25.0".to_string()),
        None,
    );

    // check that the expected deployments exist
    retry(60, Duration::from_secs(5), || {
        check_latest_identifying_attributes_match_expected(
            &server,
            &instance_id,
            expected_identifying_attributes.clone(),
        )?;
        Ok(())
    });

    let expected_identifying_attributes_sub_agent = get_expected_identifying_attributes(
        "newrelic".to_string(),
        "com.newrelic.custom_agent".to_string(),
        Some("0.0.1".to_string()),
        Some("0.25.0".to_string()),
        Some("0.0.1".to_string()),
    );

    retry(120, Duration::from_secs(5), || {
        check_latest_identifying_attributes_match_expected(
            &server,
            &instance_id_sub_agent,
            expected_identifying_attributes_sub_agent.clone(),
        )?;
        Ok(())
    })
}
