use crate::{common::retry::retry, k8s::tools::k8s_env::K8sEnv};
use assert_cmd::Command;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client, api::PostParams};
use predicates::prelude::predicate;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::runtime::Runtime;

#[test]
fn cli_install_agent_control_fails_when_no_kubernetes() {
    let mut cmd = ac_install_cmd("default", "0.0.45", "test-secret=values.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}

// NOTE: The tests below are using the latest '*' chart version and they will likely fail
// if breaking changes need to be introduced in the chart.
// If this situation occurs, we need to temporarily skip the tests or use
// a similar workaround than the one we use for e2e documented in the corresponding Tiltfile.
// Moreover, a complete installation and uninstallation test
// can be found in k8s_cli_install_agent_control_installation_and_uninstallation

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

    // The chart version does not exist
    let mut cmd = ac_install_cmd(&namespace, "0.0.0", "test-secret=values.yaml");
    cmd.assert().failure(); // The installation check should detect that the upgrade failed
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_with_invalid_image_tag() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    create_values_secret_with_invalid_image_tag(
        runtime.clone(),
        k8s_env.client.clone(),
        &namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = ac_install_cmd(&namespace, "*", "test-secret=values.yaml");
    cmd.assert().failure(); // The installation check should detect that AC workloads cannot be created due to invalid image
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

    let mut cmd = ac_install_cmd(&namespace, "*", "test-secret=values.yaml");
    cmd.assert().success(); // Install successfully

    // The chart version does not exist
    let mut cmd = ac_install_cmd(&namespace, "0.0.0", "test-secret=values.yaml");
    cmd.assert().failure(); // The installation check should detect that the upgrade failed
}

/// Builds a installation command for testing purposes with a curated set of defaults and the provided arguments.
pub fn ac_install_cmd(namespace: &str, chart_version: &str, secrets: &str) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--chart-version").arg(chart_version);
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--secrets").arg(secrets);
    cmd.arg("--installation-check-timeout").arg("1m"); // Smaller than default to speed up failure scenarios
    cmd
}

/// Create the most simple `values.yaml` secret to install AC (OpAMP disabled and empty list of agents)
pub(crate) fn create_simple_values_secret(
    runtime: Arc<Runtime>,
    client: Client,
    ns: &str,
    secret_name: &str,
    values_key: &str,
) {
    // We set cleanupManagedResources: false to avoid race conditions between the old way to uninstall and the new one
    // TODO remove it once it is not needed anymore.
    let values = serde_json::json!({
        "cleanupManagedResources": false,
        "config": {
            "fleet_control": {
                "enabled": false,
            },
            "subAgents": {
                "nr-infra": {
                    "type" : "newrelic/com.newrelic.infrastructure:0.1.0",
                    "content": {
                        "chart_version" : "*"
                    }
                },
            }
        },
        "global": {
            "cluster": "test-cluster",
            "licenseKey": "thisisafakelicensekey",
        },
    })
    .to_string();
    create_values_secret(runtime, client, ns, secret_name, values_key, values);
}

/// Create `values.yaml` secret with invalid image tag
fn create_values_secret_with_invalid_image_tag(
    runtime: Arc<Runtime>,
    client: Client,
    ns: &str,
    secret_name: &str,
    values_key: &str,
) {
    let values = serde_json::json!({
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
        "image": {"tag": "non-existent"}
    })
    .to_string();
    create_values_secret(runtime, client, ns, secret_name, values_key, values);
}

/// This helper creates a values secret with the provided `secret_name`, `values_key` and `values`.
fn create_values_secret(
    runtime: Arc<Runtime>,
    client: Client,
    ns: &str,
    secret_name: &str,
    values_key: &str,
    values: String,
) {
    let secret = Secret {
        metadata: kube::core::ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        string_data: Some(BTreeMap::from([(values_key.to_string(), values)])),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(client, ns);
    runtime
        .block_on(secrets.create(&PostParams::default(), &secret))
        .unwrap();
}
