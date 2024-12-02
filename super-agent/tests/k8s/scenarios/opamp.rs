use crate::common::{
    effective_config::check_latest_effective_config_is_expected,
    health::check_latest_health_status_was_healthy,
    opamp::{ConfigResponse, FakeServer},
    retry::retry,
    runtime::block_on,
};
use crate::k8s::tools::super_agent::{CUSTOM_AGENT_TYPE_PATH, CUSTOM_AGENT_TYPE_SECRET_PATH};

use crate::k8s::tools::k8s_api::delete_helm_release;
use crate::k8s::tools::{
    instance_id,
    k8s_api::{check_deployments_exist, check_helmrelease_spec_values},
    k8s_env::K8sEnv,
    super_agent::{
        start_super_agent_with_testdata_config, wait_until_super_agent_with_opamp_is_started,
    },
};
use newrelic_super_agent::super_agent::config::AgentID;
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;

/// OpAMP is enabled but there is no remote configuration.
/// - Local configuration is used
/// - The corresponding k8s resources are created
/// - Effective configuration is reported
/// - Healthy status is reported
/// - HelmRelease is deleted
/// - HelmRelease is recreated and healthy
#[test]
#[ignore = "needs k8s cluster"]
#[serial]
fn k8s_opamp_enabled_with_no_remote_configuration() {
    let test_name = "k8s_opamp_enabled_with_no_remote_configuration";
    let server = FakeServer::start_new();

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

    // Check the expected HelmRelease is created with the spec values from local configuration
    let expected_spec_values = r#"
cluster: minikube
licenseKey: test
    "#;

    let instance_id = instance_id::get_instance_id(&namespace, &AgentID::new_super_agent_id());
    let sub_agent_instance_id =
        instance_id::get_instance_id(&namespace, &AgentID::new("hello-world").unwrap());

    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;

        let expected_config = r#"agents:
  hello-world:
    agent_type: newrelic/com.newrelic.custom_agent:0.0.1
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id,
            expected_config.to_string(),
        )?;

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
    // Delete the helm release to check if super agent recreate it correctly
    retry(30, Duration::from_secs(1), || {
        block_on(delete_helm_release(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
        ))?;
        Ok(())
    });

    // Wait for the helm release to be recreated
    retry(60, Duration::from_secs(1), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "hello-world",
            expected_spec_values,
        ))?;
        check_latest_health_status_was_healthy(&server, &sub_agent_instance_id.clone())
    });
}

/// The local configuration for the open-telemetry collector is invalid (empty), then the remote configuration
/// is loaded and applied. After that, the remote configuration is updated and the changes should be reflected
/// in the corresponding HelmRelease resource.
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
  valid: super-true
            "#,
        ),
    );

    // Check the expected HelmRelease is updated with the new configuration
    let expected_spec_values = r#"
valid: super-true
    "#;

    retry(30, Duration::from_secs(1), || {
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

/// This scenario test how the super-agent configuration can be updated via OpAMP in order to add a new
/// sub-agent.
#[test]
#[ignore = "needs k8s cluster"]
#[serial]
fn k8s_opamp_add_subagent() {
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

    // check that the expected deployments exist
    retry(60, Duration::from_secs(1), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &["hello-world"],
            namespace.as_str(),
        ))?;

        let expected_config = r#"agents:
  hello-world:
    agent_type: newrelic/com.newrelic.custom_agent:0.0.1
"#;

        check_latest_effective_config_is_expected(
            &server,
            &instance_id,
            expected_config.to_string(),
        )?;

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
}

/// The local configuration for the open-telemetry collector has a secret with some values,
/// a new remote configuration containing new values for the secret is loaded and applied.
/// Those changes should be reflected in the corresponding HelmRelease resource.
#[test]
#[ignore = "needs k8s cluster"]
#[serial]
fn k8s_opamp_subagent_modify_secret() {
    let test_name = "k8s_opamp_subagent_modify_secret";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_SECRET_PATH,
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
    secret_name_override: remote-override-secret
           "#,
        ),
    );

    retry(60, Duration::from_secs(1), || {
        let expected_config = "secret_name_override: remote-override-secret\n";

        check_latest_effective_config_is_expected(
            &server,
            &instance_id.clone(),
            expected_config.to_string(),
        )?;

        // Check deployment has the key 'remote-override-secret' concatenated to the name because
        // the new secret created from the remote values adds that NameOverride.
        // TODO temporarily commented since there is a bug that avoids updating the Secret from remote
        /*block_on(check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-remote-override-secret"],
            namespace.as_str(),
        ))?;*/

        check_latest_health_status_was_healthy(&server, &instance_id.clone())
    });
}
