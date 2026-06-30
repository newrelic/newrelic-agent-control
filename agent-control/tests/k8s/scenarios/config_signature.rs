use crate::common::{
    retry::retry,
    runtime::{block_on, tokio_runtime},
};
use crate::k8s::tools::agent_control::CUSTOM_AGENT_TYPE_PATH;
use fake_opamp_server::FakeServer;

use crate::k8s::tools::{
    agent_control::{create_config_map, start_agent_control},
    config::K8sAgentControlConfigBuilder,
    instance_id,
    k8s_api::check_helmrelease_spec_values,
    k8s_env::K8sEnv,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use std::time::Duration;
use tempfile::tempdir;

// Most of other scenario tests are testing the agent-control with signature verification enabled.

/// Test that the agent-control can be started with signature verification disabled
/// and that it can receive and apply a configuration from OpAMP.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_signature_disabled() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agents = r#"
  hello-world:
    agent_type: "newrelic/com.newrelic.custom_agent:0.0.1"
"#;

    // start the agent-control with signature verification disabled
    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_signature_validation_disabled()
        .with_agents(agents)
        .write(k8s.client.clone(), tmp_dir.path());

    // This config is intended to be empty
    block_on(create_config_map(
        k8s.client.clone(),
        &namespace,
        "local-data-hello-world",
        "".to_string(),
    ));

    let _sa = start_agent_control(
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        tmp_dir.path(),
    );

    let instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::try_from("hello-world").unwrap(),
    );

    // Update the agent configuration via OpAMP
    server.set_config_response(
        instance_id.clone(),
        r#"
    chart_values:
      value: "from remote config"
           "#,
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
