use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;

use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use newrelic_agent_control::k8s::client::SyncK8sClient;

fn build_helm_release_command(namespace: String, release_name: String) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create")
        .arg("helm-release")
        .arg("--name")
        .arg(release_name)
        .arg("--chart-name")
        .arg("podinfo")
        .arg("--chart-version")
        .arg("6.0.0")
        .arg("--repository-name")
        .arg("podinfo-repository")
        .arg("--namespace")
        .arg(namespace);

    cmd
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_release() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name)
        .unwrap()
        .unwrap();

    assert_eq!(release.data["spec"]["interval"], "5m");
    assert_eq!(release.data["spec"]["timeout"], "5m");

    let chart_data = release.data["spec"]["chart"]["spec"].clone();
    assert_eq!(chart_data["chart"], "podinfo");
    assert_eq!(chart_data["version"], "6.0.0");
    assert_eq!(chart_data["sourceRef"]["kind"], "HelmRepository");
    assert_eq!(chart_data["sourceRef"]["name"], "podinfo-repository");
    assert_eq!(chart_data["interval"], "5m");
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_release_with_all_arguments_but_values() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.arg("--interval").arg("6m");
    cmd.arg("--timeout").arg("10m");
    cmd.arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac");
    cmd.arg("--annotations")
        .arg("test/type=integration, test/name=cli-create-helm-release");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name)
        .unwrap()
        .unwrap();

    assert_eq!(
        release.metadata.labels,
        Some(
            [("chart", "podinfo"), ("env", "testing"), ("app", "ac")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        )
    );
    assert_eq!(
        release.metadata.annotations,
        Some(
            vec![
                ("test/type", "integration"),
                ("test/name", "cli-create-helm-release")
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        )
    );
    assert_eq!(release.data["spec"]["interval"], "6m");
    assert_eq!(release.data["spec"]["timeout"], "10m");

    let chart_data = release.data["spec"]["chart"]["spec"].clone();
    assert_eq!(chart_data["chart"], "podinfo");
    assert_eq!(chart_data["version"], "6.0.0");
    assert_eq!(chart_data["sourceRef"]["kind"], "HelmRepository");
    assert_eq!(chart_data["sourceRef"]["name"], "podinfo-repository");
    assert_eq!(chart_data["interval"], "6m");
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_release_with_values() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.arg("--values").arg("key1: value1\nkey2: value2");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name)
        .unwrap()
        .unwrap();

    assert_eq!(
        release.data["spec"]["values"],
        serde_json::json!({
            "key1": "value1",
            "key2": "value2"
        })
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_release_with_values_file() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut temp_file = NamedTempFile::new().unwrap();
    let _ = temp_file.write(b"key1: value1\nkey2: value2").unwrap();

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.arg("--values-file").arg(temp_file.path());
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name)
        .unwrap()
        .unwrap();

    assert_eq!(
        release.data["spec"]["values"],
        serde_json::json!({
            "key1": "value1",
            "key2": "value2"
        })
    );
}

#[test]
fn cli_helm_release_fails_when_values_and_values_file_are_both_provided() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create").arg("helm-release");
    cmd.arg("--values").arg("");
    cmd.arg("--values-file").arg("test.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(2));
}

#[test]
fn cli_helm_release_fails_when_values_format_is_incorrect() {
    let mut cmd = build_helm_release_command("default".to_string(), "test-release".to_string());
    cmd.arg("--values").arg("key1: value1\nkey2 value2");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(65));
}

#[test]
fn cli_helm_release_fails_when_values_file_does_not_exist() {
    let mut cmd = build_helm_release_command("default".to_string(), "test-release".to_string());
    cmd.arg("--values-file").arg("nonexistent.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(66));
}
