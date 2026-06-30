use crate::common::{
    retry::retry,
    runtime::{block_on, tokio_runtime},
};
use crate::k8s::tools::agent_control::BAR_CR_AGENT_TYPE_PATH;
use crate::k8s::tools::k8s_api::check_config_map_exist;
use crate::k8s::tools::test_crd::{Foo, create_crd, delete_crd};
use crate::k8s::tools::{
    agent_control::{
        FOO_CR_AGENT_TYPE_PATH, start_agent_control, wait_until_agent_control_with_opamp_is_started,
    },
    config::K8sAgentControlConfigBuilder,
    instance_id,
    k8s_env::K8sEnv,
};
use fake_opamp_server::FakeServer;
use kube::{Api, CustomResource, CustomResourceExt};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tempfile::tempdir;

/// This scenario tests an Agent type which only create a CR when the CRD already exists.
/// The sub-agent is added from remote config and then removed.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_foo_cr_subagent() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let cr_type_meta = r#"  - apiVersion: newrelic.com/v1
    kind: Foo"#;

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_cr_type_meta(cr_type_meta)
        .write(k8s.client.clone(), tmp_dir.path());

    let _sa = start_agent_control(
        FOO_CR_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        tmp_dir.path(),
    );

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    server.set_config_response(
        instance_id.clone(),
        r#"
agents:
  foo-agent:
    agent_type: "newrelic/com.newrelic.foo_cr_agent:0.0.1"
            "#,
    );

    // Set sub-agent remote config (there is no local config and the supervisor will not start otherwise)
    let subagent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::try_from("foo-agent").unwrap(),
    );
    server.set_config_response(subagent_instance_id, "data: some-data\n");

    let api: Api<Foo> = Api::namespaced(k8s.client.clone(), &namespace);

    // Asserts the agent resources are created
    retry(120, Duration::from_secs(1), || {
        if block_on(api.get("foo-agent")).is_err() {
            return Err("foo cr not found".into());
        }
        Ok(())
    });

    // Asserts the agent resources are garbage collected
    server.set_config_response(
        instance_id.clone(),
        r#"
agents: {}
            "#,
    );

    retry(120, Duration::from_secs(1), || {
        if block_on(api.get("foo-agent")).is_ok() {
            return Err("foo found".into());
        }
        Ok(())
    });
}

/// This scenario tests an Agent type which only create a CR before CRD exists.
/// and asserts that the agent resources are created after the CRD is created.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_cr_subagent_installed_before_crd() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");
    // custom CRD defined for this test only.
    #[derive(Default, CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
    #[kube(group = "newrelic.com", version = "v1", kind = "Bar", namespaced)]
    pub struct BarSpec {
        pub data: String,
    }
    block_on(delete_crd(k8s.client.clone(), Bar::crd()))
        .expect_err("CRD deleted, testing environment was not clean, re-run the test");

    let cr_type_meta = r#"  - apiVersion: newrelic.com/v1
    kind: Bar"#;

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_cr_type_meta(cr_type_meta)
        .write(k8s.client.clone(), tmp_dir.path());

    let _sa = start_agent_control(
        BAR_CR_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        tmp_dir.path(),
    );
    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    // Set AC remote config
    server.set_config_response(
        instance_id.clone(),
        r#"
agents:
  bar-agent:
    agent_type: "newrelic/com.newrelic.bar_cr_agent:0.0.1"
            "#,
    );

    let api: Api<Bar> = Api::namespaced(k8s.client.clone(), &namespace);
    // Asserts the agent has been initialized, the config built but the resources are missing.
    retry(120, Duration::from_secs(1), || {
        block_on(check_config_map_exist(
            k8s.client.clone(),
            "fleet-data-bar-agent",
            &namespace,
        ))?;
        Ok(())
    });
    // Set sub-agent remote config (there is no local config and the supervisor will not start otherwise)
    let subagent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::try_from("bar-agent").unwrap(),
    );
    server.set_config_response(subagent_instance_id, "data: some-data\n");

    block_on(api.get("bar-agent")).expect_err("there is no Bar CRD");

    // Create the CRD
    block_on(create_crd(k8s.client.clone(), Bar::crd())).expect("Error creating the Bar CRD");

    // Asserts the agent resources are created without any intervention.
    retry(120, Duration::from_secs(1), || {
        block_on(api.get("bar-agent"))?;
        Ok(())
    });

    // clean up the CRD
    block_on(delete_crd(k8s.client.clone(), Bar::crd())).expect("Error deleting the Bar CRD");
}
