use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{
    CUSTOM_AGENT_TYPE_PATH, start_agent_control_with_testdata_config,
    wait_until_agent_control_with_opamp_is_started,
};
use crate::k8s::tools::instance_id;
use crate::k8s::tools::k8s_api::check_helmrelease_spec_values;
use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use std::time::Duration;
use tempfile::tempdir;

/// AC is started with a local configuration including a single 'hello world' such agent has an empty local
/// configuration. Then:
/// - The configuration for the sub-agent is set remotely and we check that the corresponding
///   helm-release and effective config are updated accordingly.
/// - The Agent control is restarted and we check that the helm-release and effective config keeps the values
///   remotely set.
/// - The Remote configuration is updated again and values are finally checked.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_subagent_configuration_change_after_ac_restarts() {
    let test_name = "k8s_opamp_subagent_configuration_change_after_ac_restarts";

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
      valid: true
           "#,
        ),
    );

    // Check the expected HelmRelease is created with the spec values
    let expected_spec_values = r#"
valid: true
    "#;

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_config = r#"chart_values:
  valid: true
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });

    // stop agent-control
    drop(_sa);

    // start the agent-control with the same configuration
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        // This config is intended to be empty
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    // Check that after restarting the sub-agent configuration remains as set remotely
    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_config = r#"chart_values:
  valid: true
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });

    // Update the agent configuration via OpAMP
    server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(
            r#"
chart_values:
  valid: super-super-true
            "#,
        ),
    );

    // Check the expected HelmRelease is updated with the new configuration
    let expected_spec_values = r#"
valid: super-super-true
    "#;

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_config = r#"chart_values:
  valid: super-super-true
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
}
