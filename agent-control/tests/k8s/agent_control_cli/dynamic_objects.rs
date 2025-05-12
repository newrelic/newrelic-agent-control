use assert_cmd::Command;
use newrelic_agent_control::sub_agent::identity::AgentIdentity;

use crate::k8s::tools::k8s_env::K8sEnv;
use newrelic_agent_control::agent_control::config::{
    helmrelease_v2_type_meta, helmrepository_type_meta,
};
use newrelic_agent_control::k8s::client::SyncK8sClient;

const REPOSITORY_NAME: &str = "newrelic";
const RELEASE_NAME: &str = "agent-control-deployment-release";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_creates_resources() {
    let runtime = crate::common::runtime::tokio_runtime();

    let mut k8s_env = runtime.block_on(K8sEnv::new());
    let namespace = runtime.block_on(k8s_env.test_namespace());

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg(RELEASE_NAME);
    cmd.arg("--chart-version").arg("1.0.0");
    cmd.arg("--namespace").arg(namespace.clone());
    cmd.arg("--labels")
        .arg("chart=podinfo, env=testing, app=ac");
    cmd.arg("--secrets")
        .arg("secret1=default.yaml,secret2=values.yaml,secret3=fixed.yaml");
    cmd.assert().success();

    let k8s_client = SyncK8sClient::try_new(runtime.clone(), namespace.clone()).unwrap();
    let agent_identity = AgentIdentity::new_agent_control_identity();

    // Assert repository data
    let repository = k8s_client
        .get_dynamic_object(&helmrepository_type_meta(), REPOSITORY_NAME)
        .unwrap()
        .unwrap();

    assert_eq!(
        repository.data["spec"]["url"],
        "https://helm-charts.newrelic.com"
    );
    assert_eq!(repository.data["spec"]["interval"], "300s");

    // Assert release data
    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
        .unwrap()
        .unwrap();

    assert_eq!(release.data["spec"]["interval"], "300s");
    assert_eq!(release.data["spec"]["timeout"], "300s");

    let chart_data = release.data["spec"]["chart"]["spec"].clone();
    assert_eq!(chart_data["chart"], "agent-control-deployment");
    assert_eq!(chart_data["version"], "1.0.0");
    assert_eq!(chart_data["sourceRef"]["kind"], "HelmRepository");
    assert_eq!(chart_data["sourceRef"]["name"], REPOSITORY_NAME);
    assert_eq!(chart_data["interval"], "300s");

    assert_eq!(
        release.data["spec"]["valuesFrom"],
        serde_json::json!([{
            "kind": "Secret",
            "name": "secret1",
            "valuesKey": "default.yaml",
        }, {
            "kind": "Secret",
            "name": "secret2",
            "valuesKey": "values.yaml",
        }, {
            "kind": "Secret",
            "name": "secret3",
            "valuesKey": "fixed.yaml",
        }])
    );

    // Assert labels and annotations
    assert_eq!(repository.metadata.labels, release.metadata.labels);
    assert_eq!(
        release.metadata.labels,
        Some(
            [
                ("app.kubernetes.io/managed-by", "newrelic-agent-control",),
                ("newrelic.io/agent-id", &agent_identity.id,),
                ("chart", "podinfo"),
                ("env", "testing"),
                ("app", "ac")
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
        )
    );

    assert_eq!(
        repository.metadata.annotations,
        release.metadata.annotations
    );
    assert_eq!(
        release.metadata.annotations,
        Some(
            vec![("newrelic.io/agent-type-id", agent_identity.agent_type_id)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        )
    );
}
