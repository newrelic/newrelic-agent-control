use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::retry::retry;
use crate::common::runtime::block_on;
use crate::k8s::tools::instance_id;
use crate::k8s::tools::k8s_api::check_helmrelease_spec_values;
use crate::k8s::tools::k8s_env::K8sEnv;
use crate::k8s::tools::super_agent::{
    start_super_agent_with_testdata_config, wait_until_super_agent_with_opamp_is_started,
    CUSTOM_AGENT_TYPE_PATH,
};
use newrelic_super_agent::super_agent::config::AgentID;
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;

/// The local configuration for the open-telemetry collector is invalid (empty), then the remote configuration
/// is loaded and applied. After that, the super agent is rebooted and going to check is it's capable
/// or reconnect and apply a new configuration, the remote configuration is updated and the changes
/// should be reflected in the corresponding HelmRelease resource.
#[test]
#[ignore = "needs k8s cluster"]
#[serial]
fn k8s_opamp_subagent_configuration_change() {
    let test_name = "k8s_opamp_subagent_configuration_change";

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
        // This config is intended to be empty
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    let instance_id =
        instance_id::get_instance_id(&namespace, &AgentID::new("hello-world").unwrap());

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

    retry(30, Duration::from_secs(5), || {
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

    // stop super-agent
    drop(_sa);

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &namespace,
        Some(&server.endpoint()),
        // This config is intended to be empty
        vec!["local-data-hello-world"],
        tmp_dir.path(),
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    // Update the agent configuration via OpAMP
    server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(
            r#"
chart_values:
  valid: super-true
            "#,
        ),
    );

    // Check the expected HelmRelease is updated with the new configuration
    let expected_spec_values = r#"
valid: super-true
    "#;

    retry(45, Duration::from_secs(5), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_config = r#"chart_values:
  valid: super-true
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
}
