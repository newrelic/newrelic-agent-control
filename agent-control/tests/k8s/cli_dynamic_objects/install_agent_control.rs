use std::io::Write;

use assert_cmd::Command;
use kube::api::TypeMeta;
use predicates::prelude::*;
use tempfile::NamedTempFile;

use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use newrelic_agent_control::k8s::client::SyncK8sClient;

const RELEASE_NAME: &str = "agent-control-deployment-release";
const REPOSITORY_NAME: &str = "newrelic";

fn install_agent_control_command(namespace: String) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg(RELEASE_NAME);
    cmd.arg("--chart-version").arg("1.0.0");
    cmd.arg("--namespace").arg(namespace);

    cmd
}

fn helm_repository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_creates_resources() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut cmd = install_agent_control_command(namespace.clone());
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
        .unwrap()
        .unwrap();

    assert_eq!(release.data["spec"]["interval"], "300s");
    assert_eq!(release.data["spec"]["timeout"], "300s");
    assert_eq!(release.metadata.labels, None);
    assert_eq!(release.metadata.annotations, None);

    let chart_data = release.data["spec"]["chart"]["spec"].clone();
    assert_eq!(chart_data["chart"], "agent-control");
    assert_eq!(chart_data["version"], "1.0.0");
    assert_eq!(chart_data["sourceRef"]["kind"], "HelmRepository");
    assert_eq!(chart_data["sourceRef"]["name"], REPOSITORY_NAME);
    assert_eq!(chart_data["interval"], "300s");

    let repository = k8s_client
        .get_dynamic_object(&helm_repository_type_meta(), REPOSITORY_NAME)
        .unwrap()
        .unwrap();

    assert_eq!(repository.data["spec"]["interval"], "300s");
    assert_eq!(
        repository.data["spec"]["url"],
        "https://helm-charts.newrelic.com"
    );
    assert_eq!(repository.metadata.labels, None);
    assert_eq!(repository.metadata.annotations, None);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_with_labels_and_annotations() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut cmd = install_agent_control_command(namespace.clone());
    cmd.arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac");
    cmd.arg("--annotations")
        .arg("test/type=integration, test/name=install-agent-control");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
        .unwrap()
        .unwrap();

    let repository = k8s_client
        .get_dynamic_object(&helm_repository_type_meta(), REPOSITORY_NAME)
        .unwrap()
        .unwrap();

    assert_eq!(release.metadata.labels, repository.metadata.labels);
    assert_eq!(
        release.metadata.annotations,
        repository.metadata.annotations
    );

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
                ("test/name", "install-agent-control"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        )
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_with_string_values() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut cmd = install_agent_control_command(namespace.clone());
    cmd.arg("--values").arg("key1: value1\nkey2: value2");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
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
fn k8s_cli_install_agent_control_with_file_values() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut temp_file = NamedTempFile::new().unwrap();
    let _ = temp_file.write(b"key1: value1\nkey2: value2").unwrap();

    let mut cmd = install_agent_control_command(namespace.clone());
    cmd.arg("--values")
        .arg(format!("fs://{}", temp_file.path().display()));
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();

    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
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
fn cli_helm_release_fails_when_values_format_is_incorrect() {
    let mut cmd = install_agent_control_command("default".to_string());
    cmd.arg("--values").arg("key1: value1\nkey2 value2");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(65));
}

#[test]
fn cli_helm_release_fails_when_values_file_does_not_exist() {
    let mut cmd = install_agent_control_command("default".to_string());
    cmd.arg("--values").arg("fs://nonexistent.yaml");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(66));
}
