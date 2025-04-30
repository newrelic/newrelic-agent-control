use assert_cmd::Command;
use kube::api::TypeMeta;

use crate::k8s::tools::k8s_env::K8sEnv;
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
                ("test/type", "integration"),
                ("test/name", "cli-create-helm-repository")
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        )
    );
    assert_eq!(repository.data["spec"]["interval"], "6m");
    assert_eq!(
        repository.data["spec"]["url"],
        "https://stefanprodan.github.io/podinfo"
    );
}
