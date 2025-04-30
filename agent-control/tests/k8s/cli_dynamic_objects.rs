use std::io::Write;

use assert_cmd::Command;
use kube::api::TypeMeta;
use predicates::prelude::*;
use tempfile::NamedTempFile;

use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use newrelic_agent_control::k8s::client::SyncK8sClient;

fn helm_repository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

fn build_helm_repository_command(namespace: String) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create")
        .arg("helm-repository")
        .arg("--name")
        .arg("podinfo-repository")
        .arg("--url")
        .arg("https://stefanprodan.github.io/podinfo")
        .arg("--namespace")
        .arg(namespace);

    cmd
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_repository() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let repository_name = "podinfo-repository";
    let mut cmd = build_helm_repository_command(namespace.clone());
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let repository = k8s_client
        .get_dynamic_object(&helm_repository_type_meta(), repository_name)
        .unwrap()
        .unwrap();
    assert_eq!(repository.metadata.name, Some(repository_name.to_string()));
    assert_eq!(repository.metadata.labels, None);
    assert_eq!(repository.metadata.annotations, None);
    assert_eq!(repository.data["spec"]["interval"], "5m");
    assert_eq!(
        repository.data["spec"]["url"],
        "https://stefanprodan.github.io/podinfo"
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_create_helm_repository_with_all_arguments() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let repository_name = "podinfo-repository";
    let mut cmd = build_helm_repository_command(namespace.clone());
    cmd.arg("--interval")
        .arg("6m")
        .arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac")
        .arg("--annotations")
        .arg("test/type=integration, test/name=cli-create-helm-repository");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let repository = k8s_client
        .get_dynamic_object(&helm_repository_type_meta(), repository_name)
        .unwrap()
        .unwrap();
    assert_eq!(repository.metadata.name, Some(repository_name.to_string()));
    assert_eq!(
        repository.metadata.labels,
        Some(
            [("chart", "podinfo"), ("env", "testing"), ("app", "ac")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        )
    );
    assert_eq!(
        repository.metadata.annotations,
        Some(
            vec![
                ("test/type".to_string(), "integration".to_string()),
                (
                    "test/name".to_string(),
                    "cli-create-helm-repository".to_string()
                )
            ]
            .into_iter()
            .collect()
        )
    );
    assert_eq!(repository.data["spec"]["interval"], "6m");
    assert_eq!(
        repository.data["spec"]["url"],
        "https://stefanprodan.github.io/podinfo"
    );
}

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
fn k8s_cli_create_helm_release_with_all_arguments_but_values_file() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.arg("--interval")
        .arg("6m")
        .arg("--timeout")
        .arg("10m")
        .arg("--values")
        .arg("key1: value1\nkey2: value2")
        .arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac")
        .arg("--annotations")
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
                ("test/type".to_string(), "integration".to_string()),
                (
                    "test/name".to_string(),
                    "cli-create-helm-release".to_string()
                )
            ]
            .into_iter()
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
fn k8s_cli_create_helm_release_with_all_arguments_but_values() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write(b"key1: value1\nkey2: value2").unwrap();

    let release_name = "podinfo-release";
    let mut cmd = build_helm_release_command(namespace.clone(), release_name.to_string());
    cmd.arg("--interval")
        .arg("6m")
        .arg("--timeout")
        .arg("10m")
        .arg("--values-file")
        .arg(temp_file.path())
        .arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac")
        .arg("--annotations")
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
                ("test/type".to_string(), "integration".to_string()),
                (
                    "test/name".to_string(),
                    "cli-create-helm-release".to_string()
                )
            ]
            .into_iter()
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
fn cli_helm_release_fails_when_values_and_values_file_are_both_provided() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create")
        .arg("helm-release")
        .arg("--values")
        .arg("")
        .arg("--values-file")
        .arg("test.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(2));
}

#[test]
fn cli_helm_release_fails_when_values_format_is_incorrect() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create")
        .arg("helm-release")
        .arg("--name")
        .arg("n")
        .arg("--chart-name")
        .arg("cn")
        .arg("--chart-version")
        .arg("cv")
        .arg("--repository-name")
        .arg("rn")
        .arg("--values")
        .arg("key1: value1\nkey2 value2");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(65));
}

#[test]
fn cli_helm_release_fails_when_values_file_does_not_exist() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create")
        .arg("helm-release")
        .arg("--name")
        .arg("n")
        .arg("--chart-name")
        .arg("cn")
        .arg("--chart-version")
        .arg("cv")
        .arg("--repository-name")
        .arg("rn")
        .arg("--values-file")
        .arg("nonexistent.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(66));
}
