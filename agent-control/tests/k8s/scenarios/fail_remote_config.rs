use crate::common::{
    effective_config::check_latest_effective_config_is_expected,
    opamp::{ConfigResponse, FakeServer},
    remote_config_status::check_latest_remote_config_status_is_expected,
    retry::retry,
    runtime::block_on,
};
use crate::k8s::tools::{
    agent_control::{
        start_agent_control_with_testdata_config, wait_until_agent_control_with_opamp_is_started,
    },
    instance_id,
    k8s_env::K8sEnv,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;
use tempfile::tempdir;

/// TODO
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_fail_remote_config_missing_required_values() {
    let test_name = "k8s_fail_remote_config_missing_required_values";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        format!("tests/k8s/data/{test_name}/custom_agent_type.yml").as_str(),
        k8s.client.clone(),
        &namespace,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        vec!["local-data-fake-agent"],
        tmp_dir.path(),
    );

    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::new("fake-agent").unwrap(),
    );

    server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(
            r#"
non_required_var: Configuration without required variable set
           "#,
        ),
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
