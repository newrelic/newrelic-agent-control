use super::tools::{
    k8s_api::{check_deployments_exist, check_helmrelease_spec_values},
    k8s_env::K8sEnv,
    opamp::{ConfigResponse, FakeServer},
    retry,
    runtime::block_on,
    super_agent::start_super_agent_with_testdata_config,
    uuid,
};
use crate::tools::super_agent::wait_until_super_agent_with_opamp_is_started;
use newrelic_super_agent::super_agent::config::AgentID;
use std::time::Duration;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_enabled_with_no_remote_configuration() {
    // OpAMP is enabled but there is no remote configuration.
    let test_name = "k8s_opamp_enabled_with_no_remote_configuration";
    let server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        Some(&server.endpoint()),
        vec!["local-data-open-telemetry-agent-id"],
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    // Check the expected HelmRelease is created with the spec values from local configuration
    let expected_spec_values = r#"
cluster: minikube
licenseKey: test
    "#;

    retry(30, Duration::from_secs(5), || {
        block_on(check_helmrelease_spec_values(
            k8s.client.clone(),
            namespace.as_str(),
            "open-telemetry-agent-id",
            expected_spec_values,
        ))
    });
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_subagent_configuration_change() {
    // The local configuration for the open-telemetry collector is invalid (empty), then the remote configuration
    // is loaded and applied. After that, the remote configuration is updated and the changes should be reflected
    // in the corresponding HelmRelease resource.
    let test_name = "k8s_opamp_subagent_configuration_change";

    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        Some(&server.endpoint()),
        vec!["local-data-open-telemetry-agent-id"],
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    // Update the agent configuration via OpAMP
    server.set_config_response(
        uuid::get_instance_id(
            &namespace,
            &AgentID::new("open-telemetry-agent-id").unwrap(),
        ),
        ConfigResponse::from(
            r#"
    chart_values:
      mode: deployment
      config:
        exporters:
          logging: { }
           "#,
        ),
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
        uuid::get_instance_id(
            &namespace,
            &AgentID::new("open-telemetry-agent-id").unwrap(),
        ),
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
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_opamp_add_subagent() {
    // This scenario test how the super-agent configuration can be updated via OpAMP in order to add a new
    // sub-agent.
    let test_name = "k8s_opamp_add_sub_agent";

    // setup the fake-opamp-server, with empty configuration for agents in local config local config should be used.
    let mut server = FakeServer::start_new();

    // setup the k8s environment
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());

    // start the super-agent
    let _sa = start_super_agent_with_testdata_config(
        test_name,
        k8s.client.clone(),
        &namespace,
        Some(&server.endpoint()),
        vec!["local-data-open-telemetry", "local-data-open-telemetry-2"],
    );
    wait_until_super_agent_with_opamp_is_started(k8s.client.clone(), namespace.as_str());

    // Add new agent in the super-agent configuration.
    // open-telemetry-2 will use the local config since the configuration from the server is empty
    // for io.opentelemetry.collector
    //
    // Note: This test won't work with the NewRelic k8s collector chart since the collector
    // configuration cannot yet be modified. This chart is introduced from agent type
    // version 0.2.0, so we leverage the latest agent type using the community chart.
    server.set_config_response(
        uuid::get_instance_id(&namespace, &AgentID::new_super_agent_id()),
        ConfigResponse::from(
            r#"
agents:
  open-telemetry:
    agent_type: "newrelic/io.opentelemetry.collector:0.1.1"
  open-telemetry-2:
    agent_type: "newrelic/io.opentelemetry.collector:0.1.1"
            "#,
        ),
    );

    // check that the expected deployments exist
    retry(30, Duration::from_secs(5), || {
        block_on(check_deployments_exist(
            k8s.client.clone(),
            &[
                "open-telemetry-opentelemetry-collector",
                "open-telemetry-2-opentelemetry-collector",
            ],
            namespace.as_str(),
        ))
    });
}
