use crate::common::{
    effective_config::check_latest_effective_config_is_expected,
    remote_config_status::check_latest_remote_config_status_is_expected,
    retry::retry,
    runtime::{block_on, tokio_runtime},
};
use crate::k8s::tools::{
    agent_control::{create_config_map, start_agent_control},
    config::K8sAgentControlConfigBuilder,
    instance_id,
    k8s_env::K8sEnv,
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;
use tempfile::tempdir;

const CUSTOM_AGENT_TYPE_PATH: &str =
    "tests/k8s/data/k8s_fail_remote_config_missing_required_values/custom_agent_type.yml";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_fail_remote_config_missing_required_values() {
    let mut server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agents = r#"
  fake-agent:
    agent_type: "newrelic/com.newrelic.test:0.0.1"
"#;

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_agents(agents)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &namespace,
        "local-data-fake-agent",
        "required_var: \"local\"\n".to_string(),
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
        &AgentID::try_from("fake-agent").unwrap(),
    );

    server.set_config_response(
        instance_id.clone(),
        r#"
non_required_var: Configuration without required variable set
           "#,
    );

    retry(60, Duration::from_secs(1), || {
        // Remote config Failed status
        check_latest_remote_config_status_is_expected(
            &server,
            &instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;
        // Effective config still is the local one
        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            "required_var: local\n".to_string(),
        )?;
        Ok(())
    });
}
