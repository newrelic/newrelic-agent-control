use crate::common::retry::retry;
use crate::k8s::agent_control_cli::installation::{ac_install_cmd, create_simple_values_secret};
use crate::k8s::tools::k8s_env::K8sEnv;
use assert_cmd::Command;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::Api;
use predicates::prelude::predicate;
use std::time::Duration;

#[test]
fn cli_uninstall_agent_control_fails_when_no_kubernetes() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("uninstall-agent-control");
    cmd.arg("--release-name").arg("agent-control-release");

    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_installation_and_uninstallation() {
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
    cmd.assert().success();

    let deployments: Api<Deployment> = Api::namespaced(k8s_env.client.clone(), &namespace);
    let config_maps: Api<ConfigMap> = Api::namespaced(k8s_env.client.clone(), &namespace);
    let secrets: Api<Secret> = Api::namespaced(k8s_env.client.clone(), &namespace);

    retry(10, Duration::from_secs(1), || {
        let _ = runtime.block_on(deployments.get("test-release-agent-control"))?;
        Ok(())
    });
    retry(10, Duration::from_secs(1), || {
        let _ = runtime.block_on(config_maps.get("local-data-nr-infra"))?;
        Ok(())
    });
    retry(10, Duration::from_secs(1), || {
        let _ = runtime.block_on(secrets.get("values-nr-infra"))?;
        Ok(())
    });

    let mut cmd = ac_uninstall_cmd(&namespace);
    cmd.assert().success();

    let _ = runtime
        .block_on(deployments.get("test-release-agent-control"))
        .expect_err("AC deployment should be deleted");
    let _ = runtime
        .block_on(config_maps.get("local-data-nr-infra"))
        .expect_err("SubAgent config_map should be deleted");
    let _ = runtime
        .block_on(secrets.get("values-nr-infra"))
        .expect_err("SubAgent secret should be deleted");
}

// This test can break if the chart introduces any breaking changes.
// If this situation occurs, we will need to disable the test or use
// a similar workaround than the one we use in the tiltfile.
#[test]
#[ignore = "needs k8s cluster"]
fn cli_uninstall_agent_control_fails_when_no_kubernetes_namespace() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("uninstall-agent-control");
    cmd.arg("--release-name").arg("agent-control-release");
    cmd.arg("--namespace").arg("not-existing-namespace");

    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_uninstall_agent_control_clean_empty_cluster() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut cmd = ac_uninstall_cmd(&namespace);
    cmd.assert().success();

    let mut cmd = ac_uninstall_cmd(&namespace);
    cmd.assert().success();
}

/// Builds an uninstallation command for testing purposes with a curated set of defaults and the provided arguments.
fn ac_uninstall_cmd(namespace: &str) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("uninstall-agent-control");
    cmd.arg("--release-name").arg("test-release");
    cmd.arg("--namespace").arg(namespace);
    cmd
}
