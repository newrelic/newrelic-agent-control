use crate::common::runtime::block_on;
use crate::k8s::self_update::LOCAL_CHART_REPOSITORY;
use crate::k8s::tools::cmd::{assert_stdout_contains, print_cli_output};
use crate::k8s::tools::k8s_api::create_values_secret;
use crate::k8s::tools::k8s_env::K8sEnv;
use assert_cmd::Command;
use kube::Client;
use std::time::Duration;

// NOTE: The tests below are using the latest '*' chart version, and they will likely fail
// if breaking changes need to be introduced in the chart.
// If this situation occurs, we need to temporarily skip the tests or use
// a similar workaround than the one we use for e2e documented in the corresponding Tiltfile.
// Moreover, a complete installation and uninstallation test
// can be found in k8s_cli_install_agent_control_installation_and_uninstallation

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_with_invalid_chart_version() {
    let mut k8s_env = block_on(K8sEnv::new());
    let ac_namespace = block_on(k8s_env.test_namespace());
    let subagents_namespace = block_on(k8s_env.test_namespace());

    create_simple_values_secret(
        k8s_env.client.clone(),
        &ac_namespace,
        &subagents_namespace,
        "test-secret",
        "values.yaml",
    );

    // The chart version does not exist
    let mut cmd = ac_install_cmd(&ac_namespace, "0.0.0", "test-secret=values.yaml");
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert_stdout_contains(
        &assert,
        "no 'agent-control-deployment' chart with version matching '0.0.0' found",
    );
    assert.failure(); // The installation check should detect that the upgrade failed
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_with_invalid_image_tag() {
    let mut k8s_env = block_on(K8sEnv::new());
    let ac_namespace = block_on(k8s_env.test_namespace());
    let subagents_namespace = block_on(k8s_env.test_namespace());

    create_values_secret_with_invalid_image_tag(
        k8s_env.client.clone(),
        &ac_namespace,
        &subagents_namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = ac_install_cmd(&ac_namespace, "*", "test-secret=values.yaml");
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert_stdout_contains(
        &assert,
        "Deployment `agent-control`: has 1 unavailable replicas",
    );
    assert.failure(); // The installation check should detect that AC workloads cannot be created due to invalid image
}

#[test]
#[ignore = "needs k8s cluster"]
fn podsk8s_cli_install_agent_control_installation_failed_upgrade() {
    let mut k8s_env = block_on(K8sEnv::new());
    let ac_namespace = block_on(k8s_env.test_namespace());
    let subagents_namespace = block_on(k8s_env.test_namespace());

    create_simple_values_secret(
        k8s_env.client.clone(),
        &ac_namespace,
        &subagents_namespace,
        "test-secret",
        "values.yaml",
    );

    let mut cmd = ac_install_cmd(&ac_namespace, "*", "test-secret=values.yaml");
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success(); // Install successfully

    // The chart version does not exist
    let mut cmd = ac_install_cmd(&ac_namespace, "0.0.0", "test-secret=values.yaml");
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert_stdout_contains(
        &assert,
        "no 'agent-control-deployment' chart with version matching '0.0.0' found",
    );
    assert.failure(); // The installation check should detect that the upgrade failed
}

/// Builds an installation command for testing purposes with a curated set of defaults and the provided arguments.
pub fn ac_install_cmd(namespace: &str, chart_version: &str, secrets: &str) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--log-level").arg("debug");
    cmd.arg("--chart-version").arg(chart_version);
    cmd.arg("--namespace").arg(namespace);
    cmd.arg("--secrets").arg(secrets);
    cmd.arg("--repository-url").arg(LOCAL_CHART_REPOSITORY);
    cmd.arg("--installation-check-timeout").arg("1m"); // Smaller than default to speed up failure scenarios
    cmd.timeout(Duration::from_secs(120)); // fail if the command got blocked for too long.
    cmd
}

/// Create a simple `values.yaml` secret to install AC with a single agent
pub(crate) fn create_simple_values_secret(
    client: Client,
    ac_ns: &str,
    subagents_ns: &str,
    secret_name: &str,
    values_key: &str,
) {
    let values = serde_json::json!({
        "nameOverride": "",
        "subAgentsNamespace": subagents_ns,
        "config": {
            "fleet_control": {
                "enabled": false,
            },
        },
        "global": {
            "cluster": "test-cluster",
            "licenseKey": "thisisafakelicensekey",
        },
    })
    .to_string();
    create_values_secret(client, ac_ns, secret_name, values_key, values);
}

/// Create `values.yaml` secret with invalid image tag
fn create_values_secret_with_invalid_image_tag(
    client: Client,
    ac_ns: &str,
    subagents_ns: &str,
    secret_name: &str,
    values_key: &str,
) {
    let values = serde_json::json!({
        "subAgentsNamespace": subagents_ns,
        "config": {
            "fleet_control": {
                "enabled": false,
            },
        },
        "global": {
            "cluster": "test-cluster",
            "licenseKey": "***",
        },
        "image": {"tag": "non-existent"}
    })
    .to_string();
    create_values_secret(client, ac_ns, secret_name, values_key, values);
}
