use crate::{
    common::{
        effective_config::check_latest_effective_config_is_expected,
        health::check_latest_health_status_was_healthy, opamp::FakeServer, retry::retry,
        runtime::block_on,
    },
    k8s::tools::agent_control::CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
};

use crate::k8s::tools::k8s_api::{check_helmrelease_exists, delete_helm_release};
use crate::k8s::tools::{
    agent_control::start_agent_control_with_testdata_config, instance_id,
    k8s_api::check_deployments_exist, k8s_env::K8sEnv,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use std::time::Duration;
use tempfile::tempdir;

/// Given AC with a sub-agent installed from local, opamp enabled, and different namespaces
/// for AC resources and agents. Check:
/// - Local configuration is used
/// - Effective configuration and Health is reported
/// - Sub-Agent resources are re-created in case of manual delete
/// - Sub-Agent can be removed from AC remote config
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_remove_subagent() {
    let test_name = "k8s_opamp_remove_subagent";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &ac_ns,
        &agents_ns,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    let ac_instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &ac_ns, &AgentID::new_agent_control_id());
    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::new("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-from-local"],
            agents_ns.as_str(),
        )?;

        let expected_config = r#"agents:
      hello-world:
        agent_type: newrelic/com.newrelic.custom_agent:0.0.1
    "#;

        check_latest_effective_config_is_expected(
            &server,
            &ac_instance_id,
            expected_config.to_string(),
        )?;

        check_latest_health_status_was_healthy(&server, &sub_agent_instance_id.clone())
    });

    // Delete the helm release to check if agent control recreate it correctly
    retry(30, Duration::from_secs(1), || {
        block_on(delete_helm_release(
            k8s.client.clone(),
            ac_ns.as_str(),
            "hello-world",
        ))?;
        Ok(())
    });

    // Wait for the helm release to be recreated
    // TODO this takes ~30s because is a hardcoded time in OBJECTS_SUPERVISOR_INTERVAL_SECONDS
    // we should expose this at least to a layer where we can bring it down for test.
    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_exists(
            k8s.client.clone(),
            ac_ns.as_str(),
            "hello-world",
        ))?;
        check_latest_health_status_was_healthy(&server, &sub_agent_instance_id.clone())
    });

    let remote_config = r#"
    agents: {}
    "#;

    server.set_config_response(ac_instance_id.clone(), remote_config);

    retry(60, Duration::from_secs(1), || {
        if block_on(check_helmrelease_exists(
            k8s.client.clone(),
            ac_ns.as_str(),
            "hello-world",
        ))
        .is_ok()
        {
            return Err("HelmRelease should not exist after removing the sub-agent".into());
        };

        check_latest_effective_config_is_expected(
            &server,
            &ac_instance_id.clone(),
            remote_config.to_string(),
        )
    });
}

/// This scenario test how the agent-control configuration can be updated via OpAMP in order to add a new
/// sub-agent.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_add_subagent() {
    let test_name = "k8s_opamp_add_sub_agent";

    // setup the fake-opamp-server, with empty configuration for agents in local config local config should be used.
    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let ac_ns = block_on(k8s.test_namespace());
    let agents_ns = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &ac_ns,
        &agents_ns,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    let ac_instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &ac_ns, &AgentID::new_agent_control_id());

    server.set_config_response(
        ac_instance_id.clone(),
        r#"
agents:
  hello-world:
    agent_type: "newrelic/com.newrelic.custom_agent:0.0.1"
            "#,
    );

    let sub_agent_instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &ac_ns,
        &AgentID::new("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_deployments_exist(k8s.client.clone(), &["hello-world"], &agents_ns)?;

        check_latest_effective_config_is_expected(
            &server,
            &ac_instance_id,
            r#"agents:
  hello-world:
    agent_type: newrelic/com.newrelic.custom_agent:0.0.1
"#
            .to_string(),
        )?;

        check_latest_health_status_was_healthy(&server, &sub_agent_instance_id)
    });
}

// This scenario tests how the agent-control configuration can be updated via OpAMP in order to modify an existing sub-agent configuration.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_modify_subagent_config() {
    let test_name = "k8s_opamp_modify_subagent_config";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the agent-control
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &namespace,
        &namespace,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );

    let instance_id = instance_id::get_instance_id(
        k8s.client.clone(),
        &namespace,
        &AgentID::new("hello-world").unwrap(),
    );

    retry(60, Duration::from_secs(1), || {
        check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-from-local"],
            namespace.as_str(),
        )?;

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });

    let first_remote_config = r#"
    chart_values:
        nameOverride: from-first-remote
    "#;

    server.set_config_response(instance_id.clone(), first_remote_config);

    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            first_remote_config.to_string(),
        )?;

        check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-from-first-remote"],
            namespace.as_str(),
        )?;

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });

    let second_remote_config = r#"
    chart_values:
        nameOverride: from-second-remote
    "#;

    server.set_config_response(instance_id.clone(), second_remote_config);

    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            second_remote_config.to_string(),
        )?;

        check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-from-second-remote"],
            namespace.as_str(),
        )?;

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
}
