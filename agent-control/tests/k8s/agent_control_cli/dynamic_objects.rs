use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::cmd::print_cli_output;
use crate::k8s::tools::k8s_env::K8sEnv;
use assert_cmd::Command;
use newrelic_agent_control::agent_control::config::{
    helmrelease_v2_type_meta, helmrepository_type_meta,
};
use newrelic_agent_control::cli::install_agent_control::{RELEASE_NAME, REPOSITORY_NAME};
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL};
use newrelic_agent_control::sub_agent::identity::AgentIdentity;
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_creates_resources() {
    let mut k8s_env = block_on(K8sEnv::new());
    let namespace = block_on(k8s_env.test_namespace());

    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--chart-version").arg("1.0.0");
    cmd.arg("--namespace").arg(namespace.clone());
    cmd.arg("--extra-labels")
        .arg("chart=podinfo, env=testing, app=ac");
    cmd.arg("--secrets")
        .arg("secret1=default.yaml,secret2=values.yaml,secret3=fixed.yaml");
    cmd.arg("--skip-installation-check"); // Skipping checks because we are merely checking that the resources are created.
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let agent_identity = AgentIdentity::new_agent_control_identity();

    // Assert repository data
    let repository = k8s_client
        .get_dynamic_object(&helmrepository_type_meta(), REPOSITORY_NAME, &namespace)
        .unwrap()
        .unwrap();

    let mut expected_repository = serde_json::json!({
        "url": "https://helm-charts.newrelic.com",
        "interval": "30m",
        "provider": "generic",
    });
    expected_repository = {
        expected_repository.sort_all_objects();
        ().into()
    };
    // expected_repository = expected_repository.sort_all_objects().into();

    let mut rep = repository.data["spec"].clone();
    rep = {
        rep.sort_all_objects();
        ().into()
    };

    assert_eq!(rep, expected_repository);

    // Assert release data
    let release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME, &namespace)
        .unwrap()
        .unwrap();

    let expected_release = serde_json::json!({
        "interval": "30s",
        "releaseName": RELEASE_NAME,
        "chart": {
            "spec": {
                "chart": "agent-control-deployment",
                "version": "1.0.0",
                "reconcileStrategy": "ChartVersion",
                "sourceRef": {
                    "kind": "HelmRepository",
                    "name": REPOSITORY_NAME,
                },
                "interval": "3m",
            },
        },
        "install": {
            "disableWait": true,
            "disableWaitForJobs": true,
            "disableTakeOwnership": true,
            "replace": true,
        },
        "upgrade": {
            "disableWait": true,
            "disableWaitForJobs": true,
            "disableTakeOwnership": true,
            "cleanupOnFail": true,
            "force": true,
        },
        "rollback": {
            "disableWait": true,
            "disableWaitForJobs": true
        },
        "valuesFrom": [{
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
        }],
    });
    assert_eq!(release.data["spec"], expected_release);

    let mut labels: BTreeMap<String, String> = [
        ("app.kubernetes.io/managed-by", "newrelic-agent-control"),
        ("newrelic.io/agent-id", &agent_identity.id),
        ("chart", "podinfo"),
        ("env", "testing"),
        ("app", "ac"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    // Assert labels and annotations
    assert_eq!(repository.metadata.labels, Some(labels.clone()));
    labels.insert(
        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
        LOCAL_VAL.to_string(),
    );
    // Assert labels and annotations
    assert_eq!(release.metadata.labels, Some(labels));

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

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_cli_install_agent_control_creates_resources_with_specific_repository_url() {
    let mut k8s_env = block_on(K8sEnv::new());
    let namespace = block_on(k8s_env.test_namespace());

    let repository_url = "https://cli-charts.newrelic.com";
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install-agent-control");
    cmd.arg("--chart-version").arg("1.0.0");
    cmd.arg("--namespace").arg(namespace.clone());
    cmd.arg("--skip-installation-check"); // Skipping checks because we are merely checking that the resources are created.
    cmd.arg("--repository-url").arg(repository_url);
    let assert = cmd.assert();
    print_cli_output(&assert);
    assert.success();

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let repository = k8s_client
        .get_dynamic_object(&helmrepository_type_meta(), REPOSITORY_NAME, &namespace)
        .unwrap()
        .unwrap();
    assert_eq!(repository.data["spec"]["url"], repository_url);
}
