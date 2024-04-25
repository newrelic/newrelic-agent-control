use crate::common::{
    block_on, check_deployments_exist, check_helmrelease_spec_values, retry,
    start_super_agent_with_testdata_config, K8sEnv,
};

use crate::fake_opamp::{ConfigResponse, ConfigResponses, FakeServer, Identifier};
use std::{thread::sleep as thread_sleep, time::Duration};

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_enabled_with_no_remote_configuration() {
    // OpAMP is enabled but there is no remote configuration.
    let test_name = "k8s_opamp_enabled_with_no_remote_configuration";

    // setup the fake-opamp-server
    let server_responses = ConfigResponses::from([
        (
            Identifier::from("com.newrelic.super_agent"),
            ConfigResponse::default(),
        ),
        (
            Identifier::from("io.opentelemetry.collector"),
            ConfigResponse::default(),
        ),
    ]);
    let server = FakeServer::start_new(server_responses);

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let mut sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        &server.endpoint(),
        vec!["local-data-open-telemetry-agent-id"],
    );

    // Check the expected HelmRelease is created with the spec values from local configuration
    let expected_spec_values = r#"
mode: deployment
config:
  exporters:
    logging: { }
    "#;
    retry(30, Duration::from_secs(5), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "open-telemetry-agent-id",
            expected_spec_values,
        ))
    });

    let _ = sa.kill();
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_subagent_configuration_change() {
    // The local configuration for the open-telemetry collector is invalid (empty), then the remote configuration
    // is loaded and applied. After that, the remote configuration is updated and the changes should be reflected
    // in the corresponding HelmRelease resource.
    let test_name = "k8s_opamp_subagent_configuration_change";

    // setup the fake-opamp-server
    let server_responses = ConfigResponses::from([
        (
            Identifier::from("com.newrelic.super_agent"),
            ConfigResponse::from(
                r#"
agents:
  open-telemetry-agent-id:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
            ),
        ),
        (
            Identifier::from("io.opentelemetry.collector"),
            ConfigResponse::from(
                r#"
chart_values:
  mode: deployment
  config:
    exporters:
      logging: { }
       "#,
            ),
        ),
    ]);
    let mut server = FakeServer::start_new(server_responses);

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let mut sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        &server.endpoint(),
        vec!["local-data-open-telemetry-agent-id"],
    );

    // Check the expected HelmRelease is created with the spec values
    let expected_spec_values = r#"
mode: deployment
config:
  exporters:
    logging: { }
    "#;

    retry(30, Duration::from_secs(5), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "open-telemetry-agent-id",
            expected_spec_values,
        ))
    });

    // Update the agent configuration via OpAMP
    server.set_config_response(
        Identifier::from("io.opentelemetry.collector"),
        ConfigResponse::from(
            r#"
chart_values:
  mode: deployment
  config:
    exporters:
      logging: { }
  image:
    tag: "latest"
            "#,
        ),
    );

    // Check the expected HelmRelease is updated with the new configuration
    let expected_spec_values = r#"
mode: deployment
config:
  exporters:
    logging: { }
image:
  tag: "latest"
    "#;

    retry(30, Duration::from_secs(5), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "open-telemetry-agent-id",
            expected_spec_values,
        ))
    });

    let _ = sa.kill();
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_add_subagent() {
    // This scenario test how the super-agent configuration can be updated via OpAMP in order to add a new
    // sub-agent.
    let test_name = "k8s_opamp_add_sub_agent";

    // setup the fake-opamp-server, with empty configuration for agents in local config local config should be used.
    let server_responses = ConfigResponses::from([
        (
            Identifier::from("com.newrelic.super_agent"),
            ConfigResponse::default(),
        ),
        (
            Identifier::from("io.opentelemetry.collector"),
            ConfigResponse::default(),
        ),
    ]);
    let mut server = FakeServer::start_new(server_responses);

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let mut sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        &server.endpoint(),
        vec!["local-data-open-telemetry", "local-data-open-telemetry-2"],
    );

    // Wait some time to let the super agent to be up.
    thread_sleep(Duration::from_secs(3));

    // Add new agent in the super-agent configuration.
    // open-telemetry-2 will use the local config since the configuration from the server is empty
    // for io.opentelemetry.collector
    server.set_config_response(
        Identifier::from("com.newrelic.super_agent"),
        ConfigResponse::from(
            r#"
agents:
  open-telemetry:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
  open-telemetry-2:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
            "#,
        ),
    );

    // check that the expected deployments exist
    retry(20, Duration::from_secs(5), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &[
                "open-telemetry-opentelemetry-collector",
                "open-telemetry-2-opentelemetry-collector",
            ],
            namespace.as_str(),
        ))
    });

    let _ = sa.kill();
}
