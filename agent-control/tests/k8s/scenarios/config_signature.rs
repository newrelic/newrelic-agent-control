use crate::common::{
    opamp::{ConfigResponse, FakeServer},
    retry::retry,
    runtime::block_on,
};
use crate::k8s::tools::agent_control::CUSTOM_AGENT_TYPE_PATH;

use crate::k8s::tools::{
    agent_control::{
        start_agent_control_with_testdata_config, wait_until_agent_control_with_opamp_is_started,
    },
    instance_id,
    k8s_api::check_helmrelease_spec_values,
    k8s_env::K8sEnv,
};
use newrelic_agent_control::agent_control::config::AgentID;
use std::time::Duration;
use tempfile::tempdir;

// Most of other scenario tests are testing the agent-control with signature verification enabled.

/// Test that the agent-control can be started with signature verification disabled
/// and that it can receive and apply a configuration from OpAMP.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_signature_disabled() {
    let test_name = "k8s_signature_disabled";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control with signature verification disabled
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        None,
        Some(&server.endpoint()),
        // This config is intended to be empty
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::new("hello-world").unwrap(),
    );

    // Update the agent configuration via OpAMP
    server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(
            r#"
    chart_values:
      value: "from remote config"
           "#,
        ),
    );

    // Check the expected HelmRelease is created with the remote configuration
    retry(60, Duration::from_secs(1), || {
        let expected_spec_values = r#"
value: "from remote config"
          "#;
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))
    });
}
