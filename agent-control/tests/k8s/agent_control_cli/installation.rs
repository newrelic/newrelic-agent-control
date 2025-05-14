use std::{collections::BTreeMap, sync::Arc, time::Duration};

use assert_cmd::Command;
use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::{Api, Client, api::PostParams};
use tokio::runtime::Runtime;

use crate::{common::retry::retry, k8s::tools::k8s_env::K8sEnv};

// This test can break if the chart introduces any breaking changes.
// If this situation occurs, we will need to disable the test or use
// a similar workaround than the one we use in the tiltfile.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    create_simple_values_secret(
        runtime.clone(),
        k8s_env.client.clone(),
        &namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--chart-version").arg("*"); // Use the latest version
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().success();

    let pods: Api<Pod> = Api::namespaced(k8s_env.client.clone(), &namespace);
    retry(10, Duration::from_secs(1), || {
        let _ = runtime.block_on(pods.get("test-release-agent-control"));
        Ok(())
    });
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_with_invalid_chart_version() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    create_simple_values_secret(
        runtime.clone(),
        k8s_env.client.clone(),
        &namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--chart-version").arg("0.0.0"); // This chart version does not exist
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().failure(); // The installation check should make the command fail
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_failed_upgrade() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    create_simple_values_secret(
        runtime.clone(),
        k8s_env.client.clone(),
        &namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--chart-version").arg("0.0.45"); // The version exists
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().success(); // Install successfully

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--chart-version").arg("0.0.0"); // The chart version does not exist
    cmd.arg("--namespace").arg(&namespace);
    cmd.arg("--secrets").arg("test-secret=values.yaml");
    cmd.assert().failure(); // The installation check should detect that the upgrade failed
}

/// Create the most simple `values.yaml` secret to install AC (OpAMP disabled and empty list of agents)
fn create_simple_values_secret(
    runtime: Arc<Runtime>,
    client: Client,
    ns: &str,
    secret_name: &str,
    values_key: &str,
) {
    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        string_data: Some(BTreeMap::from([(
            values_key.to_string(),
            serde_json::json!({
                "config": {
                    "fleet_control": {
                        "enabled": false,
                    },
                    "subAgents": {},
                },
                "global": {
                    "cluster": "test-cluster",
                    "licenseKey": "***",
                },
            })
            .to_string(),
        )])),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(client, ns);
    runtime
        .block_on(secrets.create(&PostParams::default(), &secret))
        .unwrap();
}
